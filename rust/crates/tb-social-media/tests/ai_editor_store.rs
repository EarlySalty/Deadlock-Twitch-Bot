#[path = "../../../test-support/postgres.rs"]
mod postgres;
use tb_social_media::{
    ai_editor::store,
    produced_video::{self, NewProducedVideo, ProducedVideoError, RenderedVideo},
    production_jobs::{self, JobCompletion, JobSource, NewProductionJob, ProductionMode},
    production_lease,
};

use sqlx::{PgPool, Row};

async fn fixture() -> postgres::TestPostgres {
    let db = postgres::TestPostgres::start().await;
    sqlx::query(
        "CREATE TABLE public.twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT NOT NULL, clip_url TEXT NOT NULL, streamer_login TEXT NOT NULL, created_at TEXT NOT NULL, twitch_user_id TEXT, category_key TEXT NOT NULL, discarded_at TIMESTAMPTZ, source_kind TEXT NOT NULL DEFAULT 'twitch' CHECK (source_kind IN ('twitch', 'manual_upload')))",
    )
    .execute(&db.pool)
    .await
    .expect("Clip-Tabelle");
    sqlx::query(
        "CREATE TABLE public.social_produced_videos (id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT NOT NULL, clip_id BIGINT NOT NULL, mode TEXT NOT NULL, version_no INTEGER NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), expires_at TIMESTAMPTZ, status TEXT NOT NULL, duration_ms BIGINT, parent_version_id BIGINT, variant_index INTEGER NOT NULL DEFAULT 0, metadata JSONB NOT NULL DEFAULT '{}'::jsonb)",
    )
    .execute(&db.pool)
    .await
    .expect("Produzierte Videos");
    sqlx::raw_sql("CREATE TABLE public.twitch_streamers (twitch_user_id TEXT PRIMARY KEY); INSERT INTO public.twitch_streamers (twitch_user_id) VALUES ('101'), ('202'); CREATE TABLE public.social_media_streamer_layout (twitch_user_id TEXT PRIMARY KEY)")
        .execute(&db.pool)
        .await
        .expect("Test-Streamer");
    sqlx::raw_sql(include_str!(
        "../../../migrations/20261011200002_social_ai_jobs.sql"
    ))
    .execute(&db.pool)
    .await
    .expect("Migration");
    sqlx::raw_sql(include_str!(
        "../../../migrations/20261011203001_social_ai_revisions.sql"
    ))
    .execute(&db.pool)
    .await
    .expect("Revisionsmigration");
    for (owner, category) in [("101", "deadlock"), ("202", "deadlock"), ("101", "other")] {
        sqlx::query(
            "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, created_at, twitch_user_id, category_key) VALUES ('clip', 'https://clips.twitch.tv/example', 'streamer', '2026-10-11T00:00:00Z', $1, $2)",
        )
        .bind(owner)
        .bind(category)
        .execute(&db.pool)
        .await
        .expect("Clip");
    }
    db
}

async fn real_schema_fixture() -> postgres::TestPostgres {
    let db = postgres::TestPostgres::start().await;
    sqlx::raw_sql(include_str!(
        "../../../migrations/20260601000000_baseline_schema.sql"
    ))
    .execute(&db.pool)
    .await
    .expect("echte Basismigration");
    sqlx::raw_sql(include_str!(
        "../../../migrations/20260629120000_live_schema_type_reconcile.sql"
    ))
    .execute(&db.pool)
    .await
    .expect("echte Typmigration");
    sqlx::query("ALTER TABLE social_media_streamer_layout ADD COLUMN twitch_user_id TEXT")
        .execute(&db.pool)
        .await
        .expect("Layout-Kanal-ID wie in 20261001090000");
    sqlx::query("INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('fixture-one', '101'), ('fixture-two', '202'), ('earlysalty', '303')")
        .execute(&db.pool)
        .await
        .expect("isolierte Streamer");
    sqlx::raw_sql(include_str!(
        "../../../migrations/20260806120000_social_media_partner_access.sql"
    ))
    .execute(&db.pool)
    .await
    .expect("echte Partnerzugangsmigration");
    sqlx::raw_sql(include_str!(
        "../../../migrations/20260815120000_social_media_scheduling.sql"
    ))
    .execute(&db.pool)
    .await
    .expect("echte Kategoriemigration");
    sqlx::raw_sql(include_str!(
        "../../../migrations/20261011100000_social_autopilot_contracts.sql"
    ))
    .execute(&db.pool)
    .await
    .expect("echter M0-Vertrag");
    sqlx::raw_sql(include_str!(
        "../../../migrations/20261011200002_social_ai_jobs.sql"
    ))
    .execute(&db.pool)
    .await
    .expect("echte KI-Auftragsmigration");
    sqlx::raw_sql(include_str!(
        "../../../migrations/20261011203001_social_ai_revisions.sql"
    ))
    .execute(&db.pool)
    .await
    .expect("echte Revisionsmigration");
    sqlx::query("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, created_at, twitch_user_id, category_key) VALUES ('fixture-clip-101', 'https://clips.twitch.tv/fixture-101', 'fixture-one', '2026-10-11T00:00:00Z', '101', 'deadlock'), ('fixture-clip-202', 'https://clips.twitch.tv/fixture-202', 'fixture-two', '2026-10-11T00:00:00Z', '202', 'deadlock')")
        .execute(&db.pool)
        .await
        .expect("isolierte Clips");
    db
}

fn limit() -> store::AttemptLimit {
    store::AttemptLimit::from_server_config(2).expect("Limit")
}

fn payload(json: &str) -> serde_json::Map<String, serde_json::Value> {
    serde_json::from_str(json).expect("Objekt")
}

fn preparation() -> store::StageCheckpoint {
    store::StageCheckpoint::Preparing {
        source_path: "/private/source.mp4".to_string(),
        original_duration_ms: Some(35_000),
        snapshot: payload(r#"{"source_id":"clip","layout_version":1}"#),
    }
}

fn transcription() -> store::StageCheckpoint {
    store::StageCheckpoint::Transcribing {
        transcript: "Originalsprache".to_string(),
        language: Some("de".to_string()),
        segments: vec![store::TranscriptSegment {
            start_ms: 100,
            end_ms: 900,
            text: "Originalsprache".to_string(),
        }],
    }
}

fn planning() -> store::StageCheckpoint {
    store::StageCheckpoint::Planning {
        decisions: vec![store::TimelineEntry {
            start_ms: 100,
            end_ms: 900,
            description: "Ausschnitt".to_string(),
        }],
        review_reasons: Vec::new(),
        edit_plan: payload(r#"{"cuts":[{"start_ms":100,"end_ms":900}]}"#),
    }
}

async fn start_job(pool: &PgPool) -> store::AiJobView {
    store::create_job(pool, "101", 1, limit())
        .await
        .expect("Eigener Deadlock-Clip")
}

async fn step(lease: &mut store::ProductionLease, checkpoint: store::StageCheckpoint) {
    store::claim_job(lease, "101", 1, limit())
        .await
        .expect("Schritt beanspruchen");
    store::save_stage(lease, "101", 1, &checkpoint, None)
        .await
        .expect("Checkpoint speichern");
}

#[tokio::test]
async fn real_m0_schema_keeps_manual_identity_and_allocates_atomic_metadata() {
    let db = real_schema_fixture().await;
    let column_types: Vec<(String, String)> = sqlx::query_as(
        "SELECT a.attrelid::regclass::text || '.' || a.attname, format_type(a.atttypid, a.atttypmod) \
         FROM pg_attribute a WHERE (a.attrelid, a.attname) IN ( \
         ('twitch_clips_social_media'::regclass, 'id'), \
         ('twitch_clips_social_media'::regclass, 'clip_id'), \
         ('social_ai_jobs'::regclass, 'source_clip_id'), \
         ('social_produced_videos'::regclass, 'clip_id')) ORDER BY 1",
    )
    .fetch_all(&db.pool)
    .await
    .expect("echte Spaltentypen");
    assert_eq!(column_types.len(), 4);
    assert_eq!(column_types[0].1, "bigint");
    assert_eq!(column_types[1].1, "bigint");
    assert_eq!(column_types[2].1, "text");
    assert_eq!(column_types[3].1, "bigint");
    let settings = tb_social_media::autopilot_settings::load(&db.pool, "101")
        .await
        .expect("echte C1-Einstellungen");
    assert!(!settings.enabled && !settings.ai_editing_enabled);
    let mut paused_settings = settings;
    paused_settings.paused = true;
    tb_social_media::autopilot_settings::save(&db.pool, "101", &paused_settings)
        .await
        .expect("deaktivierter und pausierter Auto-Pilot");
    assert!(!store::get_user_consent(&db.pool, "101")
        .await
        .expect("fehlende Einwilligung"));
    store::upsert_user_consent(&db.pool, "101", true, "fixture-owner")
        .await
        .expect("echter Eigentümer-Consent");
    assert!(store::get_user_consent(&db.pool, "101")
        .await
        .expect("eigene Einwilligung"));
    assert!(!store::get_user_consent(&db.pool, "202")
        .await
        .expect("anderer Kanal"));
    let job = store::create_job(&db.pool, "101", 1, limit())
        .await
        .expect("manueller Auftrag trotz deaktiviertem Auto-Pilot");
    assert!(matches!(
        store::create_job(&db.pool, "202", 1, limit()).await,
        Err(store::AiStoreError::NotFound)
    ));
    let spec = NewProducedVideo {
        twitch_user_id: "101".into(),
        clip_id: 1,
        job_id: None,
        mode: ProductionMode::Ai,
        version_no: 1,
        parent_version_id: None,
        variant_index: 0,
    };
    let metadata = serde_json::json!({"manual_ai_job_id": job.id.to_string()});
    assert!(matches!(
        produced_video::insert_with_metadata(&db.pool, &spec, &serde_json::json!([])).await,
        Err(ProducedVideoError::Invalid)
    ));
    let video = produced_video::insert_with_metadata(&db.pool, &spec, &metadata)
        .await
        .expect("atomare C2-Zuordnung");
    assert_eq!(video.metadata, metadata);
    assert_eq!(video.job_id, None);
    assert_eq!(video.status, "rendering");
    assert_eq!(
        store::get_job(&db.pool, "101", job.id, limit())
            .await
            .expect("Ansicht")
            .versions
            .len(),
        1
    );
    assert!(matches!(
        produced_video::insert_with_metadata(&db.pool, &spec, &metadata).await,
        Err(ProducedVideoError::Database(_))
    ));
    let mut other = spec.clone();
    other.version_no = 2;
    other.twitch_user_id = "202".into();
    assert!(matches!(
        produced_video::insert_with_metadata(&db.pool, &other, &metadata).await,
        Err(ProducedVideoError::Ownership)
    ));
    other.twitch_user_id = "101".into();
    other.parent_version_id = Some(video.id);
    let child = produced_video::insert(&db.pool, &other)
        .await
        .expect("bestehender Insert-Pfad mit gültigem Parent");
    assert_eq!(child.metadata, serde_json::json!({}));
    assert_eq!(child.parent_version_id, Some(video.id));
    let mut variant = spec.clone();
    variant.variant_index = 1;
    assert_eq!(
        produced_video::insert_with_metadata(&db.pool, &variant, &metadata)
            .await
            .expect("zweite Variante derselben Versionsnummer")
            .version_no,
        1
    );
    other.version_no = 1;
    assert!(matches!(
        produced_video::insert(&db.pool, &other).await,
        Err(ProducedVideoError::Ownership)
    ));
    let directory = tempfile::tempdir().expect("temporärer Renderordner");
    let file = directory.path().join("render.mp4");
    tokio::fs::write(&file, b"fixture video")
        .await
        .expect("temporäre Videodatei");
    let rendered = produced_video::complete_render(
        &db.pool,
        "101",
        video.id,
        &RenderedVideo {
            file_path: file.to_str().expect("Videopfad").into(),
            duration_ms: 900,
            needs_review: true,
            review_reasons: Vec::new(),
            metadata: serde_json::json!({"manual_ai_job_id": "forged", "platforms": {"youtube": {"title": "Fixture"}}}),
        },
    )
    .await
    .expect("Renderfreigabe");
    assert_eq!(rendered.metadata["manual_ai_job_id"], job.id.to_string());
    assert_eq!(
        rendered.metadata["platforms"]["youtube"]["title"],
        "Fixture"
    );
    let view = store::get_job(&db.pool, "101", job.id, limit())
        .await
        .expect("Version nach Rendering");
    assert_eq!(view.versions.len(), 2);
    assert_eq!(view.versions[0].status, "review");
    assert_eq!(view.versions[1].version_no, 1);
    assert!(matches!(
        store::get_job(&db.pool, "202", job.id, limit()).await,
        Err(store::AiStoreError::NotFound)
    ));
    let foreign_job = production_jobs::insert(
        &db.pool,
        &NewProductionJob {
            twitch_user_id: "202".into(),
            clip_id: 2,
            source: JobSource::Manual,
            requested_mode: Some(ProductionMode::Ai),
            scheduled_at: None,
            max_attempts: 2,
        },
    )
    .await
    .expect("eigener C3-Auftrag für anderen Kanal");
    let claimed = production_jobs::claim_next(&db.pool)
        .await
        .expect("C3-Claim")
        .expect("wartender Auftrag");
    assert_eq!(claimed.id, foreign_job.id);
    let mut linked = spec.clone();
    linked.version_no = 3;
    linked.job_id = Some(foreign_job.id);
    assert!(matches!(
        produced_video::insert(&db.pool, &linked).await,
        Err(ProducedVideoError::Ownership)
    ));
    production_jobs::finish(
        &db.pool,
        "202",
        foreign_job.id,
        JobCompletion::Completed,
        &[],
        None,
    )
    .await
    .expect("fremden C3-Auftrag abschließen");
    let own_job = production_jobs::insert(
        &db.pool,
        &NewProductionJob {
            twitch_user_id: "101".into(),
            clip_id: 1,
            source: JobSource::Manual,
            requested_mode: Some(ProductionMode::Ai),
            scheduled_at: None,
            max_attempts: 2,
        },
    )
    .await
    .expect("echter C3-Auftrag");
    assert_eq!(
        production_jobs::claim_next(&db.pool)
            .await
            .expect("C3-Claim")
            .expect("eigener Auftrag")
            .id,
        own_job.id
    );
    linked.job_id = Some(own_job.id);
    linked.mode = ProductionMode::Static;
    assert!(matches!(
        produced_video::insert(&db.pool, &linked).await,
        Err(ProducedVideoError::Ownership)
    ));
    linked.mode = ProductionMode::Ai;
    assert_eq!(
        produced_video::insert(&db.pool, &linked)
            .await
            .expect("gültiger C3-Bezug")
            .job_id,
        Some(own_job.id)
    );
    assert!(matches!(
        sqlx::query("DELETE FROM twitch_clips_social_media WHERE id = 1")
            .execute(&db.pool)
            .await,
        Err(sqlx::Error::Database(error)) if error.code().as_deref() == Some("23503")
    ));
}

#[tokio::test]
async fn ownership_category_and_concurrent_active_create_are_scoped() {
    let db = fixture().await;
    let (first, second) = tokio::join!(
        store::create_job(&db.pool, "101", 1, limit()),
        store::create_job(&db.pool, "101", 1, limit())
    );
    let job_id = first.expect("erster Start").id;
    assert_eq!(job_id, second.expect("zweiter Start").id);
    assert!(matches!(
        store::create_job(&db.pool, "202", 1, limit()).await,
        Err(store::AiStoreError::NotFound)
    ));
    assert!(matches!(
        store::create_job(&db.pool, "101", 3, limit()).await,
        Err(store::AiStoreError::NotFound)
    ));
    assert!(matches!(
        store::get_job(&db.pool, "202", job_id, limit()).await,
        Err(store::AiStoreError::NotFound)
    ));
    assert!(matches!(
        store::retry_job(&db.pool, "202", job_id, limit()).await,
        Err(store::AiStoreError::NotFound)
    ));
    let own = store::create_job(&db.pool, "202", 2, limit())
        .await
        .expect("eigener Clip");
    assert_eq!(own.clip_db_id, 2);
    assert_eq!(
        store::list_jobs(&db.pool, "101", 500, None, limit())
            .await
            .expect("Liste")
            .len(),
        1
    );
    assert!(store::list_jobs(&db.pool, "101", 10, Some(job_id), limit())
        .await
        .expect("Seite")
        .is_empty());
    let serialized = serde_json::to_string(
        &store::get_job(&db.pool, "101", job_id, limit())
            .await
            .expect("Job"),
    )
    .expect("öffentliche Ansicht");
    assert!(!serialized.contains("source_path"));
    assert!(!serialized.contains("transcript"));
    assert!(!serialized.contains("twitch_user_id"));
    assert!(store::AttemptLimit::from_server_config(0).is_err());
    sqlx::query("DELETE FROM twitch_clips_social_media WHERE id = 1")
        .execute(&db.pool)
        .await
        .expect("Clip-Retention darf nicht blockiert werden");
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("Lease")
        .expect("frei");
    assert!(matches!(
        store::load_job_for_runner(&mut lease, "101", job_id).await,
        Err(store::AiStoreError::SourceUnavailable)
    ));
    let retained = store::get_job(&db.pool, "101", job_id, limit())
        .await
        .expect("Auftragshistorie bleibt erhalten");
    assert_eq!(retained.clip_db_id, 1);
    assert_eq!(retained.status, "failed");
    assert_eq!(retained.failure_code.as_deref(), Some("source_unavailable"));
    assert!(!retained.can_retry);
    let source_clip_id: Option<i64> =
        sqlx::query_scalar("SELECT source_clip_id FROM social_ai_jobs WHERE id = $1")
            .bind(job_id)
            .fetch_one(&db.pool)
            .await
            .expect("abgelöster Fremdschlüssel");
    assert_eq!(source_clip_id, None);
}

#[tokio::test]
async fn retry_preserves_analysis_when_rendering_fails_and_finish_needs_real_version() {
    let db = fixture().await;
    start_job(&db.pool).await;
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("Leasesystem")
        .expect("freie Lease");
    step(&mut lease, preparation()).await;
    step(&mut lease, transcription()).await;
    step(
        &mut lease,
        store::StageCheckpoint::Analyzing {
            highlights: vec![store::TimelineEntry {
                start_ms: 100,
                end_ms: 900,
                description: "Kampf".to_string(),
            }],
            uncertainty: vec!["Spieler nicht sicher erkannt".to_string()],
            analysis_notes: vec!["Kontext".to_string()],
        },
    )
    .await;
    step(&mut lease, planning()).await;
    store::claim_job(&mut lease, "101", 1, limit())
        .await
        .expect("Rendering");
    assert!(matches!(
        store::claim_job(&mut lease, "101", 1, limit()).await,
        Err(store::AiStoreError::Conflict)
    ));
    store::fail_job(
        &mut lease,
        "101",
        1,
        store::AiStage::Rendering,
        store::JobFailure::RenderingFailed,
        None,
    )
    .await
    .expect("Renderfehler");
    let failed = store::get_job(&db.pool, "101", 1, limit())
        .await
        .expect("Fehleransicht");
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.stage, "rendering");
    assert_eq!(failed.highlights.len(), 1);
    assert_eq!(failed.decisions.len(), 1);
    assert_eq!(failed.original_duration_ms, Some(35_000));
    assert_eq!(failed.estimated_provider_cost_usd, None);
    assert_eq!(failed.measured_provider_cost_usd, None);
    assert_eq!(failed.cost_basis, "unknown");
    assert!(failed.can_retry);
    lease.release().await.expect("Lease freigeben");
    store::retry_job(&db.pool, "101", 1, limit())
        .await
        .expect("Retry");
    assert!(matches!(
        store::retry_job(&db.pool, "101", 1, limit()).await,
        Err(store::AiStoreError::Conflict)
    ));
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("Lease")
        .expect("frei");
    let snapshot = store::load_job_for_runner(&mut lease, "101", 1)
        .await
        .expect("Checkpoint");
    assert_eq!(snapshot.stage, store::AiStage::Rendering);
    assert!(snapshot
        .checkpoints
        .contains_key(&store::AiStage::Analyzing));
    assert!(snapshot
        .checkpoints
        .contains_key(&store::AiStage::Transcribing));
    match snapshot.checkpoints.get(&store::AiStage::Planning) {
        Some(store::StageCheckpoint::Planning { edit_plan, .. }) => {
            assert_eq!(
                edit_plan,
                &payload(r#"{"cuts":[{"start_ms":100,"end_ms":900}]}"#)
            );
        }
        _ => panic!("Schnittplan fehlt"),
    }
    match snapshot.checkpoints.get(&store::AiStage::Transcribing) {
        Some(store::StageCheckpoint::Transcribing { segments, .. }) => {
            assert_eq!(segments[0].start_ms, 100);
            assert_eq!(segments[0].end_ms, 900);
        }
        _ => panic!("Zeitsegmente fehlen"),
    }
    assert!(!snapshot
        .checkpoints
        .contains_key(&store::AiStage::Rendering));
    store::claim_job(&mut lease, "101", 1, limit())
        .await
        .expect("zweiter Versuch");
    store::save_stage(
        &mut lease,
        "101",
        1,
        &store::StageCheckpoint::Rendering {
            output_path: "/private/rendered.mp4".to_string(),
            output_duration_ms: Some(800),
            rendering_ms: 2400,
        },
        None,
    )
    .await
    .expect("Renderingcheckpoint");
    store::claim_job(&mut lease, "101", 1, limit())
        .await
        .expect("Validierung");
    store::save_stage(
        &mut lease,
        "101",
        1,
        &store::StageCheckpoint::Validating {
            review_reasons: vec!["Schnitt bitte prüfen".to_string()],
        },
        None,
    )
    .await
    .expect("Validierungscheckpoint");
    assert!(matches!(
        store::finish_job(&mut lease, "101", 1, 999).await,
        Err(store::AiStoreError::Conflict)
    ));
    sqlx::query("INSERT INTO social_produced_videos (id, twitch_user_id, clip_id, mode, version_no, status, duration_ms, metadata) VALUES (9, '101', 1, 'ai', 1, 'review', 800, '{\"manual_ai_job_id\": 1}')")
        .execute(&db.pool).await.expect("tatsächliche Version");
    store::finish_job(&mut lease, "101", 1, 9)
        .await
        .expect("Abschluss");
    assert!(matches!(
        store::finish_job(&mut lease, "101", 1, 9).await,
        Err(store::AiStoreError::Conflict)
    ));
    let reviewed = store::get_job(&db.pool, "101", 1, limit())
        .await
        .expect("Sichtung");
    assert_eq!(reviewed.status, "review");
    assert_eq!(reviewed.versions.len(), 1);
    assert_eq!(reviewed.versions[0].file_url, None);
    assert_eq!(reviewed.original_url, None);
    assert_eq!(reviewed.rendering_ms, Some(2400));
    assert!(!reviewed.can_retry);
    sqlx::query("DELETE FROM twitch_clips_social_media WHERE id = 1")
        .execute(&db.pool)
        .await
        .expect("Retention abgeschlossener Clips");
    let retained = store::get_job(&db.pool, "101", 1, limit())
        .await
        .expect("Abschluss bleibt erhalten");
    assert_eq!(retained.status, "review");
    assert_eq!(retained.clip_db_id, 1);
    assert_eq!(retained.decisions.len(), 1);
    assert_eq!(retained.completed_at, reviewed.completed_at);
    assert!(!retained.can_retry);
}

#[tokio::test]
async fn failed_checkpoint_write_does_not_advance_state() {
    let db = fixture().await;
    start_job(&db.pool).await;
    sqlx::raw_sql(
        "CREATE FUNCTION reject_stage_checkpoint() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.checkpoints ? 'preparing' THEN RAISE EXCEPTION 'synthetic database write failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER reject_stage_checkpoint BEFORE UPDATE ON social_ai_jobs FOR EACH ROW EXECUTE FUNCTION reject_stage_checkpoint();",
    )
    .execute(&db.pool)
    .await
    .expect("Schreibfehler-Fixierung");
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("Lease")
        .expect("frei");
    store::claim_job(&mut lease, "101", 1, limit())
        .await
        .expect("Claim");
    assert!(matches!(
        store::save_stage(&mut lease, "101", 1, &preparation(), None).await,
        Err(store::AiStoreError::Database(_))
    ));
    let snapshot = store::load_job_for_runner(&mut lease, "101", 1)
        .await
        .expect("unveränderter Job");
    assert_eq!(snapshot.status, "preparing");
    assert_eq!(snapshot.stage, store::AiStage::Preparing);
    assert!(snapshot.checkpoints.is_empty());
}

#[tokio::test]
async fn consent_lease_release_recovery_and_estimated_cost_are_explicit() {
    let db = fixture().await;
    assert!(!store::get_user_consent(&db.pool, "101")
        .await
        .expect("Standard"));
    store::upsert_user_consent(&db.pool, "101", true, "owner")
        .await
        .expect("Einwilligung");
    assert!(store::get_user_consent(&db.pool, "101")
        .await
        .expect("Einwilligung lesen"));
    assert!(!store::get_user_consent(&db.pool, "202")
        .await
        .expect("anderer Kanal"));
    store::upsert_user_consent(&db.pool, "101", false, "owner")
        .await
        .expect("Widerruf");
    assert!(!store::get_user_consent(&db.pool, "101")
        .await
        .expect("Widerruf lesen"));
    start_job(&db.pool).await;
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("Lease")
        .expect("frei");
    assert!(store::acquire_production_lease(&db.pool)
        .await
        .expect("zweiter Versuch")
        .is_none());
    store::claim_job(&mut lease, "101", 1, limit())
        .await
        .expect("Claim");
    let metrics = store::JobMetrics {
        cumulative_provider_totals: Some(store::CumulativeProviderTotals {
            provider: Some("configured-provider".to_string()),
            model: Some("configured-model".to_string()),
            input_tokens: Some(10),
            output_tokens: Some(5),
            estimated_provider_cost_usd: Some(0.042),
            ..Default::default()
        }),
        ..Default::default()
    };
    store::save_stage(&mut lease, "101", 1, &preparation(), Some(&metrics))
        .await
        .expect("Schritt");
    let view = store::get_job(&db.pool, "101", 1, limit())
        .await
        .expect("Ansicht");
    assert_eq!(view.cost_basis, "usage_priced_estimate");
    assert_eq!(view.measured_provider_cost_usd, None);
    assert_eq!(view.input_tokens, Some(10));
    assert!(!serde_json::to_string(&view)
        .expect("JSON")
        .contains("/private/"));
    store::claim_job(&mut lease, "101", 1, limit())
        .await
        .expect("nächster Schritt");
    store::save_stage(
        &mut lease,
        "101",
        1,
        &transcription(),
        Some(&store::JobMetrics {
            processing_ms: Some(500),
            ..Default::default()
        }),
    )
    .await
    .expect("Teilmetriken speichern");
    let view = store::get_job(&db.pool, "101", 1, limit())
        .await
        .expect("Gesamtkosten bleiben erhalten");
    assert_eq!(view.estimated_provider_cost_usd, Some(0.042));
    assert_eq!(view.input_tokens, Some(10));
    assert_eq!(view.processing_ms, Some(500));
    store::claim_job(&mut lease, "101", 1, limit())
        .await
        .expect("weiterer Schritt");
    assert!(matches!(
        store::recover_stale_jobs(&mut lease, "101", 10).await,
        Err(store::AiStoreError::Conflict)
    ));
    assert_eq!(
        store::load_job_for_runner(&mut lease, "101", 1)
            .await
            .expect("laufender Auftrag")
            .status,
        "analyzing"
    );
    lease.release().await.expect("Lease freigeben");
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("erneute Lease")
        .expect("freigegeben");
    assert_eq!(
        store::recover_stale_jobs(&mut lease, "101", 10)
            .await
            .expect("Wiederherstellung"),
        1
    );
    let snapshot = store::load_job_for_runner(&mut lease, "101", 1)
        .await
        .expect("erhaltener Stand");
    assert_eq!(snapshot.status, "failed");
    assert_eq!(snapshot.stage, store::AiStage::Analyzing);
    assert!(snapshot
        .checkpoints
        .contains_key(&store::AiStage::Transcribing));
    assert_eq!(
        store::recover_stale_jobs(&mut lease, "101", 10)
            .await
            .expect("idempotent"),
        0
    );
    let row = sqlx::query("SELECT status FROM social_ai_jobs WHERE id = 1")
        .fetch_one(&db.pool)
        .await
        .expect("Datenbankzustand");
    assert_eq!(
        row.try_get::<String, _>("status").expect("Status"),
        "failed"
    );
}

#[tokio::test]
async fn source_changes_fail_closed_at_load_claim_and_creation() {
    let db = fixture().await;
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("Lease")
        .expect("frei");
    for (id, mutation) in [
        (
            4,
            "UPDATE twitch_clips_social_media SET twitch_user_id = '202' WHERE id = 4",
        ),
        (
            5,
            "UPDATE twitch_clips_social_media SET category_key = 'other' WHERE id = 5",
        ),
        (
            6,
            "UPDATE twitch_clips_social_media SET discarded_at = NOW() WHERE id = 6",
        ),
        (
            7,
            "UPDATE twitch_clips_social_media SET source_kind = 'manual_upload' WHERE id = 7",
        ),
        (8, "DELETE FROM twitch_clips_social_media WHERE id = 8"),
    ] {
        sqlx::query("INSERT INTO twitch_clips_social_media (id, clip_id, clip_url, streamer_login, created_at, twitch_user_id, category_key) VALUES ($1, 'clip', 'https://clips.twitch.tv/example', 'streamer', '2026-10-11T00:00:00Z', '101', 'deadlock')")
            .bind(id)
            .execute(&db.pool)
            .await
            .expect("Clip vorbereiten");
        let job = store::create_job(&db.pool, "101", id.into(), limit())
            .await
            .expect("berechtigter Start");
        sqlx::query(mutation)
            .execute(&db.pool)
            .await
            .expect("Quellenänderung");
        if id == 8 {
            assert_eq!(
                store::recover_stale_jobs(&mut lease, "101", 10)
                    .await
                    .expect("gelöschte Warteschlange"),
                1
            );
        }
        assert!(matches!(
            store::load_job_for_runner(&mut lease, "101", job.id).await,
            Err(store::AiStoreError::SourceUnavailable)
        ));
        let view = store::get_job(&db.pool, "101", job.id, limit())
            .await
            .expect("sichtbarer Fehler");
        assert_eq!(view.status, "failed");
        assert!(!view.can_retry);
        assert!(matches!(
            store::retry_job(&db.pool, "101", job.id, limit()).await,
            Err(store::AiStoreError::Conflict)
        ));
    }
    sqlx::query("UPDATE twitch_clips_social_media SET category_key = 'deadlock' WHERE id = 5")
        .execute(&db.pool)
        .await
        .expect("Kategorie wiederherstellen");
    let recovered_id: i64 =
        sqlx::query_scalar("SELECT id FROM social_ai_jobs WHERE clip_db_id = 5")
            .fetch_one(&db.pool)
            .await
            .expect("Auftragskennung");
    let recovered = store::get_job(&db.pool, "101", recovered_id, limit())
        .await
        .expect("wieder berechtigter Clip");
    assert!(recovered.can_retry);
    store::retry_job(&db.pool, "101", recovered.id, limit())
        .await
        .expect("nach Korrektur wiederholbar");
    store::claim_job(&mut lease, "101", recovered.id, limit())
        .await
        .expect("erneut beanspruchbar");
    sqlx::query("INSERT INTO twitch_clips_social_media (id, clip_id, clip_url, streamer_login, created_at, twitch_user_id, category_key) VALUES (9, 'clip', 'https://clips.twitch.tv/example', 'streamer', '2026-10-11T00:00:00Z', '101', 'deadlock')")
        .execute(&db.pool)
        .await
        .expect("zweiter Clip");
    let job = store::create_job(&db.pool, "101", 9, limit())
        .await
        .expect("Job vor Claim");
    sqlx::query("UPDATE twitch_clips_social_media SET source_kind = 'manual_upload' WHERE id = 9")
        .execute(&db.pool)
        .await
        .expect("geänderte Quelle");
    assert!(matches!(
        store::claim_job(&mut lease, "101", job.id, limit()).await,
        Err(store::AiStoreError::SourceUnavailable)
    ));
    let view = store::get_job(&db.pool, "101", job.id, limit())
        .await
        .expect("Claim-Fehler");
    assert_eq!(view.failure_code.as_deref(), Some("source_unavailable"));
    assert!(matches!(
        store::create_job(&db.pool, "101", 9, limit()).await,
        Err(store::AiStoreError::NotFound)
    ));
    sqlx::query("UPDATE twitch_clips_social_media SET source_kind = 'twitch', discarded_at = NOW() WHERE id = 9")
        .execute(&db.pool)
        .await
        .expect("verworfener Clip");
    assert!(matches!(
        store::create_job(&db.pool, "101", 9, limit()).await,
        Err(store::AiStoreError::NotFound)
    ));
}

#[tokio::test]
async fn cumulative_totals_replace_unknown_and_compute_only_preserves_them() {
    let db = fixture().await;
    start_job(&db.pool).await;
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("Lease")
        .expect("frei");
    store::claim_job(&mut lease, "101", 1, limit())
        .await
        .expect("Vorbereitung");
    store::save_stage(
        &mut lease,
        "101",
        1,
        &preparation(),
        Some(&store::JobMetrics {
            cumulative_provider_totals: Some(store::CumulativeProviderTotals {
                input_tokens: Some(10),
                output_tokens: Some(5),
                estimated_provider_cost_usd: Some(0.042),
                ..Default::default()
            }),
            ..Default::default()
        }),
    )
    .await
    .expect("bekannte Summe");
    store::claim_job(&mut lease, "101", 1, limit())
        .await
        .expect("Transkription");
    store::save_stage(
        &mut lease,
        "101",
        1,
        &transcription(),
        Some(&store::JobMetrics {
            cumulative_provider_totals: Some(store::CumulativeProviderTotals::default()),
            ..Default::default()
        }),
    )
    .await
    .expect("unbekannte Summe");
    let unknown = store::get_job(&db.pool, "101", 1, limit())
        .await
        .expect("unbekannte Kosten");
    assert_eq!(unknown.input_tokens, None);
    assert_eq!(unknown.estimated_provider_cost_usd, None);
    assert_eq!(unknown.cost_basis, "unknown");
    store::claim_job(&mut lease, "101", 1, limit())
        .await
        .expect("Analyse");
    store::save_stage(
        &mut lease,
        "101",
        1,
        &store::StageCheckpoint::Analyzing {
            highlights: Vec::new(),
            uncertainty: Vec::new(),
            analysis_notes: Vec::new(),
        },
        Some(&store::JobMetrics {
            cumulative_provider_totals: Some(store::CumulativeProviderTotals {
                input_tokens: Some(15),
                output_tokens: Some(9),
                estimated_provider_cost_usd: Some(0.052),
                ..Default::default()
            }),
            ..Default::default()
        }),
    )
    .await
    .expect("kumulative Summe ohne Addition");
    store::claim_job(&mut lease, "101", 1, limit())
        .await
        .expect("Planung");
    store::save_stage(
        &mut lease,
        "101",
        1,
        &planning(),
        Some(&store::JobMetrics {
            processing_ms: Some(500),
            ..Default::default()
        }),
    )
    .await
    .expect("nur Rechenzeit");
    let view = store::get_job(&db.pool, "101", 1, limit())
        .await
        .expect("Summenprüfung");
    assert_eq!(view.input_tokens, Some(15));
    assert_eq!(view.output_tokens, Some(9));
    assert_eq!(view.estimated_provider_cost_usd, Some(0.052));
    assert_eq!(view.measured_provider_cost_usd, None);
    assert_eq!(view.processing_ms, Some(500));
}

#[tokio::test]
async fn no_speech_and_bounded_segments_and_plan_roundtrip() {
    let db = fixture().await;
    start_job(&db.pool).await;
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("Lease")
        .expect("frei");
    step(&mut lease, preparation()).await;
    store::claim_job(&mut lease, "101", 1, limit())
        .await
        .expect("Transkription");
    let invalid = store::StageCheckpoint::Transcribing {
        transcript: "Wort".to_string(),
        language: None,
        segments: vec![
            store::TranscriptSegment {
                start_ms: 100,
                end_ms: 300,
                text: "Wort".to_string(),
            },
            store::TranscriptSegment {
                start_ms: 200,
                end_ms: 500,
                text: "weiter".to_string(),
            },
        ],
    };
    assert!(matches!(
        store::save_stage(&mut lease, "101", 1, &invalid, None).await,
        Err(store::AiStoreError::InvalidInput)
    ));
    assert!(matches!(
        store::save_stage(
            &mut lease,
            "101",
            1,
            &store::StageCheckpoint::Transcribing {
                transcript: "  ".to_string(),
                language: None,
                segments: Vec::new(),
            },
            None
        )
        .await,
        Err(store::AiStoreError::InvalidInput)
    ));
    store::save_stage(
        &mut lease,
        "101",
        1,
        &store::StageCheckpoint::Transcribing {
            transcript: String::new(),
            language: None,
            segments: Vec::new(),
        },
        None,
    )
    .await
    .expect("echte Stille");
    step(
        &mut lease,
        store::StageCheckpoint::Analyzing {
            highlights: Vec::new(),
            uncertainty: Vec::new(),
            analysis_notes: Vec::new(),
        },
    )
    .await;
    store::claim_job(&mut lease, "101", 1, limit())
        .await
        .expect("Planung");
    assert!(matches!(
        store::save_stage(
            &mut lease,
            "101",
            1,
            &store::StageCheckpoint::Planning {
                decisions: Vec::new(),
                review_reasons: Vec::new(),
                edit_plan: serde_json::Map::new(),
            },
            None
        )
        .await,
        Err(store::AiStoreError::InvalidInput)
    ));
    store::save_stage(&mut lease, "101", 1, &planning(), None)
        .await
        .expect("strukturierter Plan");
    let snapshot = store::load_job_for_runner(&mut lease, "101", 1)
        .await
        .expect("persistierte Stufen");
    match snapshot.checkpoints.get(&store::AiStage::Transcribing) {
        Some(store::StageCheckpoint::Transcribing {
            transcript,
            language,
            segments,
        }) => {
            assert!(transcript.is_empty());
            assert!(language.is_none());
            assert!(segments.is_empty());
        }
        _ => panic!("Stille verloren"),
    }
    match snapshot.checkpoints.get(&store::AiStage::Planning) {
        Some(store::StageCheckpoint::Planning { edit_plan, .. }) => {
            assert_eq!(
                edit_plan,
                &payload(r#"{"cuts":[{"start_ms":100,"end_ms":900}]}"#)
            );
        }
        _ => panic!("Plan verloren"),
    }
    let public = serde_json::to_string(
        &store::get_job(&db.pool, "101", 1, limit())
            .await
            .expect("öffentliche Ansicht"),
    )
    .expect("öffentliche Daten");
    assert!(!public.contains("cuts"));
    assert!(!public.contains("source_id"));
}

#[tokio::test]
async fn failed_provider_attempt_keeps_authoritative_cumulative_totals() {
    let db = fixture().await;
    start_job(&db.pool).await;
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("Lease")
        .expect("frei");
    step(&mut lease, preparation()).await;
    step(&mut lease, transcription()).await;
    store::claim_job(&mut lease, "101", 1, limit())
        .await
        .expect("Analyse");
    store::fail_job(
        &mut lease,
        "101",
        1,
        store::AiStage::Analyzing,
        store::JobFailure::ProviderUnavailable,
        Some(&store::JobMetrics {
            processing_ms: Some(750),
            cumulative_provider_totals: Some(store::CumulativeProviderTotals {
                input_tokens: Some(28),
                output_tokens: None,
                estimated_provider_cost_usd: None,
                ..Default::default()
            }),
            ..Default::default()
        }),
    )
    .await
    .expect("Versuch trotz Fehler gespeichert");
    let view = store::get_job(&db.pool, "101", 1, limit())
        .await
        .expect("Kosten des fehlgeschlagenen Versuchs");
    assert_eq!(view.status, "failed");
    assert_eq!(view.input_tokens, Some(28));
    assert_eq!(view.output_tokens, None);
    assert_eq!(view.processing_ms, Some(750));
    assert_eq!(view.cost_basis, "unknown");
    assert!(view.can_retry);
}

#[tokio::test]
async fn twitch_source_identifier_is_frozen_without_rejecting_metadata_refresh() {
    let db = fixture().await;
    let job = start_job(&db.pool).await;
    let source_id: String =
        sqlx::query_scalar("SELECT source_twitch_clip_id FROM social_ai_jobs WHERE id = $1")
            .bind(job.id)
            .fetch_one(&db.pool)
            .await
            .expect("gespeicherte Twitch-Kennung");
    assert_eq!(source_id, "clip");
    sqlx::query("UPDATE twitch_clips_social_media SET clip_url = 'https://clips.twitch.tv/refreshed', streamer_login = 'updated' WHERE id = 1")
        .execute(&db.pool)
        .await
        .expect("Metadaten aktualisieren");
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("Lease")
        .expect("frei");
    store::claim_job(&mut lease, "101", job.id, limit())
        .await
        .expect("unveränderte Quelle beanspruchen");
    store::save_stage(&mut lease, "101", job.id, &preparation(), None)
        .await
        .expect("Vorbereitung über Metadatenänderung hinweg");
    sqlx::query("UPDATE twitch_clips_social_media SET clip_id = 'replacement' WHERE id = 1")
        .execute(&db.pool)
        .await
        .expect("Clip-Kennung ersetzen");
    assert!(matches!(
        store::create_job(&db.pool, "101", 1, limit()).await,
        Err(store::AiStoreError::NotFound)
    ));
    assert!(matches!(
        store::claim_job(&mut lease, "101", job.id, limit()).await,
        Err(store::AiStoreError::SourceUnavailable)
    ));
    assert!(matches!(
        store::load_job_for_runner(&mut lease, "101", job.id).await,
        Err(store::AiStoreError::SourceUnavailable)
    ));
    let old = store::get_job(&db.pool, "101", job.id, limit())
        .await
        .expect("erhaltene Auftragshistorie");
    assert_eq!(old.status, "failed");
    assert_eq!(old.failure_code.as_deref(), Some("source_unavailable"));
    assert!(!old.can_retry);
    assert!(matches!(
        store::retry_job(&db.pool, "101", job.id, limit()).await,
        Err(store::AiStoreError::Conflict)
    ));
    let saved: bool =
        sqlx::query_scalar("SELECT checkpoints ? 'preparing' FROM social_ai_jobs WHERE id = $1")
            .bind(job.id)
            .fetch_one(&db.pool)
            .await
            .expect("Checkpoint bleibt erhalten");
    assert!(saved);
    lease.release().await.expect("Lease freigeben");
    let replacement = store::create_job(&db.pool, "101", 1, limit())
        .await
        .expect("neue Quelle unter gleicher Datenbankkennung");
    assert_ne!(replacement.id, job.id);
    let replacement_id: String =
        sqlx::query_scalar("SELECT source_twitch_clip_id FROM social_ai_jobs WHERE id = $1")
            .bind(replacement.id)
            .fetch_one(&db.pool)
            .await
            .expect("Kennung der neuen Quelle");
    assert_eq!(replacement_id, "replacement");
}

#[tokio::test]
async fn twitch_identifier_swap_blocks_inflight_save_and_finish() {
    let db = fixture().await;
    let first = start_job(&db.pool).await;
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("Lease")
        .expect("frei");
    store::claim_job(&mut lease, "101", first.id, limit())
        .await
        .expect("Vorbereitung beanspruchen");
    sqlx::query("UPDATE twitch_clips_social_media SET clip_id = 'replacement' WHERE id = 1")
        .execute(&db.pool)
        .await
        .expect("Kennung während der Vorbereitung ändern");
    assert!(matches!(
        store::save_stage(&mut lease, "101", first.id, &preparation(), None).await,
        Err(store::AiStoreError::Conflict)
    ));
    assert!(matches!(
        store::load_job_for_runner(&mut lease, "101", first.id).await,
        Err(store::AiStoreError::SourceUnavailable)
    ));
    lease.release().await.expect("Lease freigeben");
    let second = store::create_job(&db.pool, "101", 1, limit())
        .await
        .expect("neuer Auftrag für neue Quelle");
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("Lease")
        .expect("frei");
    for checkpoint in [
        preparation(),
        transcription(),
        store::StageCheckpoint::Analyzing {
            highlights: Vec::new(),
            uncertainty: Vec::new(),
            analysis_notes: Vec::new(),
        },
        planning(),
        store::StageCheckpoint::Rendering {
            output_path: "/private/rendered.mp4".to_string(),
            output_duration_ms: Some(800),
            rendering_ms: 2400,
        },
        store::StageCheckpoint::Validating {
            review_reasons: Vec::new(),
        },
    ] {
        store::claim_job(&mut lease, "101", second.id, limit())
            .await
            .expect("Schritt beanspruchen");
        store::save_stage(&mut lease, "101", second.id, &checkpoint, None)
            .await
            .expect("Schritt speichern");
    }
    sqlx::query("INSERT INTO social_produced_videos (twitch_user_id, clip_id, mode, version_no, status, metadata) VALUES ('101', 1, 'ai', 1, 'review', jsonb_build_object('manual_ai_job_id', $1::text))")
        .bind(second.id)
        .execute(&db.pool)
        .await
        .expect("gebundene Video-Version");
    sqlx::query("UPDATE twitch_clips_social_media SET clip_id = 'another' WHERE id = 1")
        .execute(&db.pool)
        .await
        .expect("Kennung vor Abschluss ändern");
    assert!(matches!(
        store::finish_job(&mut lease, "101", second.id, 1).await,
        Err(store::AiStoreError::Conflict)
    ));
    assert!(matches!(
        store::load_job_for_runner(&mut lease, "101", second.id).await,
        Err(store::AiStoreError::SourceUnavailable)
    ));
    let retained = store::get_job(&db.pool, "101", second.id, limit())
        .await
        .expect("Abschluss wurde verweigert");
    assert_eq!(retained.status, "failed");
    assert_eq!(retained.failure_code.as_deref(), Some("source_unavailable"));
    assert!(!retained.can_retry);
    assert_eq!(retained.versions.len(), 1);
}

#[tokio::test]
async fn loaded_active_job_blocks_same_lease_recovery_but_terminal_read_does_not() {
    let db = fixture().await;
    let job = start_job(&db.pool).await;
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("Lease")
        .expect("frei");
    store::claim_job(&mut lease, "101", job.id, limit())
        .await
        .expect("aktive Vorbereitung");
    lease.release().await.expect("Lease freigeben");
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("neu erworbene Lease")
        .expect("frei");
    let snapshot = store::load_job_for_runner(&mut lease, "101", job.id)
        .await
        .expect("aktiven Auftrag laden");
    assert_eq!(snapshot.status, "preparing");
    assert!(matches!(
        store::recover_stale_jobs(&mut lease, "101", 10).await,
        Err(store::AiStoreError::Conflict)
    ));
    assert_eq!(
        store::get_job(&db.pool, "101", job.id, limit())
            .await
            .expect("aktiver Auftrag bleibt aktiv")
            .status,
        "preparing"
    );
    lease.release().await.expect("Lease freigeben");
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("erneut erworbene Lease")
        .expect("frei");
    assert_eq!(
        store::recover_stale_jobs(&mut lease, "101", 10)
            .await
            .expect("verwaiste Arbeit zurücksetzen"),
        1
    );
    sqlx::query("INSERT INTO twitch_clips_social_media (id, clip_id, clip_url, streamer_login, created_at, twitch_user_id, category_key) VALUES (4, 'other-clip', 'https://clips.twitch.tv/other', 'streamer', '2026-10-11T00:00:00Z', '101', 'deadlock')")
        .execute(&db.pool)
        .await
        .expect("zweiten Clip anlegen");
    let another = store::create_job(&db.pool, "101", 4, limit())
        .await
        .expect("weiterer Auftrag");
    store::claim_job(&mut lease, "101", another.id, limit())
        .await
        .expect("weiterer aktiver Auftrag");
    lease.release().await.expect("Lease freigeben");
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("nächste Lease")
        .expect("frei");
    let terminal = store::load_job_for_runner(&mut lease, "101", job.id)
        .await
        .expect("Fehlerhistorie lesen");
    assert_eq!(terminal.status, "failed");
    assert_eq!(
        store::recover_stale_jobs(&mut lease, "101", 10)
            .await
            .expect("andere verwaiste Arbeit zurücksetzen"),
        1
    );
    assert_eq!(
        store::get_job(&db.pool, "101", another.id, limit())
            .await
            .expect("zurückgesetzter Auftrag")
            .failure_code
            .as_deref(),
        Some("interrupted")
    );
}

#[tokio::test]
async fn public_pages_group_bounded_versions_without_exposing_checkpoints() {
    let db = fixture().await;
    let original = start_job(&db.pool).await;
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("Lease")
        .expect("frei");
    step(&mut lease, preparation()).await;
    lease.release().await.expect("Lease freigeben");
    sqlx::query("INSERT INTO twitch_clips_social_media (id, clip_id, clip_url, streamer_login, created_at, twitch_user_id, category_key) SELECT n, 'clip-' || n, 'https://clips.twitch.tv/example', 'streamer', '2026-10-11T00:00:00Z', '101', 'deadlock' FROM generate_series(4, 53) AS n")
        .execute(&db.pool)
        .await
        .expect("50 zusätzliche Clips");
    sqlx::query("INSERT INTO social_ai_jobs (twitch_user_id, clip_db_id, source_clip_id, source_twitch_clip_id) SELECT 101, id, id, clip_id FROM twitch_clips_social_media WHERE id BETWEEN 4 AND 53 ORDER BY id")
        .execute(&db.pool)
        .await
        .expect("50 zusätzliche Aufträge");
    let newest_id: i64 = sqlx::query_scalar("SELECT id FROM social_ai_jobs WHERE clip_db_id = 53")
        .fetch_one(&db.pool)
        .await
        .expect("jüngster Auftrag");
    let next_id: i64 = sqlx::query_scalar("SELECT id FROM social_ai_jobs WHERE clip_db_id = 52")
        .fetch_one(&db.pool)
        .await
        .expect("zweiter Auftrag");
    sqlx::query("INSERT INTO social_produced_videos (twitch_user_id, clip_id, mode, version_no, status, metadata) SELECT '101', 53, 'ai', n, 'review', jsonb_build_object('manual_ai_job_id', $1::text) FROM generate_series(1, 55) AS n")
        .bind(newest_id)
        .execute(&db.pool)
        .await
        .expect("Versionen über dem Limit");
    sqlx::query("INSERT INTO social_produced_videos (twitch_user_id, clip_id, mode, version_no, status, metadata) VALUES ('101', 52, 'ai', 2, 'review', jsonb_build_object('manual_ai_job_id', $1::text)), ('101', 52, 'ai', 1, 'review', jsonb_build_object('manual_ai_job_id', $1::text)), ('202', 53, 'ai', 56, 'review', jsonb_build_object('manual_ai_job_id', $2::text)), ('101', 53, 'manual', 57, 'review', jsonb_build_object('manual_ai_job_id', $2::text)), ('101', 53, 'ai', 58, 'review', jsonb_build_object('manual_ai_job_id', $1::text)), ('101', 52, 'ai', 59, 'review', jsonb_build_object('manual_ai_job_id', $2::text))")
        .bind(next_id)
        .bind(newest_id)
        .execute(&db.pool)
        .await
        .expect("abzugrenzende Versionen");
    let first = store::list_jobs(&db.pool, "101", 500, None, limit())
        .await
        .expect("erste Seite");
    assert_eq!(first.len(), 50);
    assert_eq!(first[0].id, newest_id);
    assert_eq!(first[49].id, original.id + 1);
    assert_eq!(first[0].versions.len(), 50);
    assert_eq!(first[0].versions[0].version_no, 1);
    assert_eq!(first[0].versions[49].version_no, 50);
    assert_eq!(first[1].id, next_id);
    assert_eq!(first[1].versions.len(), 2);
    assert_eq!(first[1].versions[0].version_no, 1);
    assert_eq!(first[1].versions[1].version_no, 2);
    let second = store::list_jobs(&db.pool, "101", 50, Some(first[49].id), limit())
        .await
        .expect("zweite Seite");
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].id, original.id);
    let detail = store::get_job(&db.pool, "101", newest_id, limit())
        .await
        .expect("öffentlicher Detailaufruf");
    assert_eq!(detail.versions.len(), 50);
    let serialized =
        serde_json::to_string(&(first, second, detail)).expect("öffentliche JSON-Daten");
    assert!(!serialized.contains("/private/source.mp4"));
    assert!(!serialized.contains("source_path"));
    assert!(!serialized.contains("checkpoints"));
    assert!(!serialized.contains("source_twitch_clip_id"));
    let other = store::list_jobs(&db.pool, "202", 50, None, limit())
        .await
        .expect("anderer Kanal");
    assert!(other.is_empty());
}

#[tokio::test]
async fn real_schema_recovery_identity_deletion_and_private_projection() {
    let db = real_schema_fixture().await;
    let job = store::create_job(&db.pool, "101", 1, limit())
        .await
        .expect("manueller Auftrag");
    let mut lease = production_lease::acquire_production_lease(&db.pool)
        .await
        .expect("Sperre")
        .expect("freie Sperre");
    assert!(production_lease::acquire_production_lease(&db.pool)
        .await
        .expect("Konkurrenz")
        .is_none());
    store::claim_job(&mut lease, "101", job.id, limit())
        .await
        .expect("Vorbereitung");
    store::save_stage(&mut lease, "101", job.id, &preparation(), None)
        .await
        .expect("privater Checkpoint");
    store::claim_job(&mut lease, "101", job.id, limit())
        .await
        .expect("Transkription");
    let loaded = store::load_job_for_runner(&mut lease, "101", job.id)
        .await
        .expect("laufende Arbeit");
    assert_eq!(loaded.status, "transcribing");
    assert!(matches!(
        store::recover_stale_jobs(&mut lease, "101", 10).await,
        Err(store::AiStoreError::Conflict)
    ));
    for public in [
        store::get_job(&db.pool, "101", job.id, limit())
            .await
            .expect("öffentliche Details"),
        store::list_jobs(&db.pool, "101", 10, None, limit())
            .await
            .expect("öffentliche Liste")
            .remove(0),
    ] {
        let json = serde_json::to_string(&public).expect("öffentliche Projektion");
        assert!(!json.contains("/private/source.mp4"));
        assert!(!json.contains("checkpoints"));
        assert!(!json.contains("source_twitch_clip_id"));
    }
    lease.release().await.expect("Sperre freigeben");
    let mut lease = production_lease::acquire_production_lease(&db.pool)
        .await
        .expect("neue Sperre")
        .expect("frei");
    assert_eq!(
        store::recover_stale_jobs(&mut lease, "101", 10)
            .await
            .expect("verwaiste Arbeit"),
        1
    );
    assert_eq!(
        store::load_job_for_runner(&mut lease, "101", job.id)
            .await
            .expect("Endzustand")
            .status,
        "failed"
    );
    assert_eq!(
        store::recover_stale_jobs(&mut lease, "101", 10)
            .await
            .expect("terminaler Lesezugriff blockiert nicht"),
        0
    );
    lease.release().await.expect("Sperre freigeben");
    sqlx::query("DELETE FROM twitch_clips_social_media WHERE id = 1")
        .execute(&db.pool)
        .await
        .expect("echter FK erlaubt Retention");
    let source: Option<i64> =
        sqlx::query_scalar("SELECT source_clip_id FROM social_ai_jobs WHERE id = $1")
            .bind(job.id)
            .fetch_one(&db.pool)
            .await
            .expect("gelöster Fremdschlüssel");
    assert_eq!(source, None);
    assert_eq!(
        store::get_job(&db.pool, "101", job.id, limit())
            .await
            .expect("Historie")
            .clip_db_id,
        1
    );
    assert!(matches!(
        store::retry_job(&db.pool, "101", job.id, limit()).await,
        Err(store::AiStoreError::Conflict)
    ));
    let other = store::create_job(&db.pool, "202", 2, limit())
        .await
        .expect("weiterer Kanal");
    sqlx::query("UPDATE twitch_clips_social_media SET clip_id = 'changed-identity' WHERE id = 2")
        .execute(&db.pool)
        .await
        .expect("Twitch-Kennung geändert");
    let mut lease = production_lease::acquire_production_lease(&db.pool)
        .await
        .expect("Sperre")
        .expect("frei");
    assert!(matches!(
        store::claim_job(&mut lease, "202", other.id, limit()).await,
        Err(store::AiStoreError::SourceUnavailable)
    ));
    assert!(
        !store::get_job(&db.pool, "202", other.id, limit())
            .await
            .expect("Identität unverändert")
            .can_retry
    );
}

#[tokio::test]
async fn neutral_production_lease_releases_on_task_cancellation() {
    let db = real_schema_fixture().await;
    let pool = db.pool.clone();
    let (held_tx, held_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _lease = production_lease::acquire_production_lease(&pool)
            .await
            .expect("Sperre")
            .expect("frei");
        held_tx.send(()).expect("Startsignal");
        std::future::pending::<()>().await;
    });
    held_rx.await.expect("Sperre ist aktiv");
    assert!(production_lease::acquire_production_lease(&db.pool)
        .await
        .expect("konkurrierender Produktionslauf")
        .is_none());
    task.abort();
    assert!(task
        .await
        .expect_err("abgebrochener Auftrag")
        .is_cancelled());
    let mut released = None;
    for _ in 0..1000 {
        released = production_lease::acquire_production_lease(&db.pool)
            .await
            .expect("erneuter Sperrversuch");
        if released.is_some() {
            break;
        }
        tokio::task::yield_now().await;
    }
    released
        .expect("Abbruch schließt die Sperrverbindung")
        .release()
        .await
        .expect("ausdrückliche Freigabe");
}
