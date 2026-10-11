#[path = "../../../test-support/postgres.rs"]
mod postgres;
#[path = "../src/ai_editor/store.rs"]
pub mod store;

use sqlx::{PgPool, Row};

async fn fixture() -> postgres::TestPostgres {
    let db = postgres::TestPostgres::start().await;
    sqlx::query(
        "CREATE TABLE public.twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT, category_key TEXT NOT NULL, discarded_at TIMESTAMPTZ)",
    )
    .execute(&db.pool)
    .await
    .expect("Clip-Tabelle");
    sqlx::query(
        "CREATE TABLE public.social_produced_videos (id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT NOT NULL, clip_id BIGINT NOT NULL, mode TEXT NOT NULL, version_no INTEGER NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), status TEXT NOT NULL, duration_ms BIGINT, parent_version_id BIGINT, metadata JSONB NOT NULL DEFAULT '{}'::jsonb)",
    )
    .execute(&db.pool)
    .await
    .expect("Produzierte Videos");
    sqlx::query(
        "CREATE TABLE public.social_ai_user_consent (twitch_user_id TEXT PRIMARY KEY, external_ai_consent BOOLEAN NOT NULL DEFAULT FALSE, updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), updated_by TEXT NOT NULL)",
    )
    .execute(&db.pool)
    .await
    .expect("Einwilligungstabelle");
    sqlx::raw_sql(include_str!(
        "../../../migrations/20261011200002_social_ai_jobs.sql"
    ))
    .execute(&db.pool)
    .await
    .expect("Migration");
    for (owner, category) in [("101", "deadlock"), ("202", "deadlock"), ("101", "other")] {
        sqlx::query(
            "INSERT INTO twitch_clips_social_media (twitch_user_id, category_key) VALUES ($1, $2)",
        )
        .bind(owner)
        .bind(category)
        .execute(&db.pool)
        .await
        .expect("Clip");
    }
    db
}

fn limit() -> store::AttemptLimit {
    store::AttemptLimit::from_server_config(2).expect("Limit")
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
async fn ownership_category_and_concurrent_active_create_are_scoped() {
    let db = fixture().await;
    let (first, second) = tokio::join!(
        store::create_job(&db.pool, "101", 1, limit()),
        store::create_job(&db.pool, "101", 1, limit())
    );
    assert_eq!(
        first.expect("erster Start").id,
        second.expect("zweiter Start").id
    );
    assert!(matches!(
        store::create_job(&db.pool, "202", 1, limit()).await,
        Err(store::AiStoreError::NotFound)
    ));
    assert!(matches!(
        store::create_job(&db.pool, "101", 3, limit()).await,
        Err(store::AiStoreError::NotFound)
    ));
    assert!(matches!(
        store::get_job(&db.pool, "202", 1, limit()).await,
        Err(store::AiStoreError::NotFound)
    ));
    assert!(matches!(
        store::retry_job(&db.pool, "202", 1, limit()).await,
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
    assert!(store::list_jobs(&db.pool, "101", 10, Some(1), limit())
        .await
        .expect("Seite")
        .is_empty());
    let serialized = serde_json::to_string(
        &store::get_job(&db.pool, "101", 1, limit())
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
    assert!(matches!(
        store::get_job(&db.pool, "101", 1, limit()).await,
        Err(store::AiStoreError::NotFound)
    ));
}

#[tokio::test]
async fn retry_preserves_analysis_when_rendering_fails_and_finish_needs_real_version() {
    let db = fixture().await;
    start_job(&db.pool).await;
    let mut lease = store::acquire_production_lease(&db.pool)
        .await
        .expect("Leasesystem")
        .expect("freie Lease");
    step(
        &mut lease,
        store::StageCheckpoint::Preparing {
            source_path: "/private/source.mp4".to_string(),
            original_duration_ms: Some(35_000),
        },
    )
    .await;
    step(
        &mut lease,
        store::StageCheckpoint::Transcribing {
            transcript: "Originalsprache".to_string(),
            language: "de".to_string(),
        },
    )
    .await;
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
    step(
        &mut lease,
        store::StageCheckpoint::Planning {
            decisions: vec![store::TimelineEntry {
                start_ms: 100,
                end_ms: 900,
                description: "Ausschnitt".to_string(),
            }],
            review_reasons: Vec::new(),
        },
    )
    .await;
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
        store::save_stage(
            &mut lease,
            "101",
            1,
            &store::StageCheckpoint::Preparing {
                source_path: "/private/source.mp4".to_string(),
                original_duration_ms: Some(1_000),
            },
            None
        )
        .await,
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
        provider: Some("configured-provider".to_string()),
        model: Some("configured-model".to_string()),
        input_tokens: Some(10),
        output_tokens: Some(5),
        estimated_provider_cost_usd: Some(0.042),
        ..Default::default()
    };
    store::save_stage(
        &mut lease,
        "101",
        1,
        &store::StageCheckpoint::Preparing {
            source_path: "/private/source.mp4".to_string(),
            original_duration_ms: Some(1_000),
        },
        Some(&metrics),
    )
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
        &store::StageCheckpoint::Transcribing {
            transcript: "Originalsprache".to_string(),
            language: "de".to_string(),
        },
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
