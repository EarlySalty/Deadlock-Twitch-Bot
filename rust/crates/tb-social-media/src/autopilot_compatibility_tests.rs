use std::{borrow::Cow, path::PathBuf, str::FromStr, time::Duration};

use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};

use crate::{
    approval,
    autopilot_settings::{self, AutopilotSettings, ScheduleSlot},
    produced_video::{self, NewProducedVideo, RenderedVideo},
    production_jobs::ProductionMode,
    retention,
    retention_worker::RetentionWorker,
};

const M0: i64 = 20261011100000;
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

fn options() -> PgConnectOptions {
    PgConnectOptions::from_str(&crate::test_support::test_dsn().expect("isolated PostgreSQL"))
        .expect("test connection options")
}

async fn connection_pool(schema: &str, label: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(6)
        .connect_with(
            options()
                .options([("search_path", schema)])
                .application_name(label),
        )
        .await
        .expect("isolated fixture connections")
}

async fn fixture(schema: &str) -> PgPool {
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options())
        .await
        .expect("isolated admin");
    sqlx::query(crate::test_sql::drop_schema(schema, true))
        .execute(&admin)
        .await
        .expect("drop fixture");
    sqlx::query(crate::test_sql::create_schema(schema, false))
        .execute(&admin)
        .await
        .expect("create fixture");
    admin.close().await;
    let pool = connection_pool(schema, schema).await;
    sqlx::raw_sql("CREATE TABLE twitch_streamers (twitch_user_id TEXT PRIMARY KEY);
        INSERT INTO twitch_streamers VALUES ('owner'), ('other');
        CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT, clip_id TEXT NOT NULL,
            clip_url TEXT NOT NULL, streamer_login TEXT NOT NULL, source_kind TEXT DEFAULT 'twitch',
            upload_local_path TEXT, local_file_path TEXT, preview_path TEXT, converted_file_path TEXT, clip_title TEXT,
            status TEXT DEFAULT 'pending', created_at TIMESTAMPTZ DEFAULT NOW(), retention_until TIMESTAMPTZ,
            discarded_at TIMESTAMPTZ, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_tiktok BOOLEAN DEFAULT FALSE,
            uploaded_instagram BOOLEAN DEFAULT FALSE);
        CREATE TABLE social_media_platform_auth (platform TEXT, twitch_user_id TEXT, streamer_login TEXT, enabled INTEGER DEFAULT 1);
        CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT DEFAULT 'awaiting_approval',
            approved_platforms JSONB DEFAULT '[]', approver_user_id TEXT, decided_at TIMESTAMPTZ,
            dm_message_id TEXT, dm_channel_id TEXT, last_sent_at TIMESTAMPTZ, letzter_nachreih_versuch TIMESTAMPTZ);
        CREATE TABLE twitch_clips_upload_queue (id BIGSERIAL PRIMARY KEY, clip_id BIGINT, platform TEXT,
            status TEXT DEFAULT 'pending', scheduled_at TIMESTAMPTZ, priority INTEGER DEFAULT 0,
            title TEXT, description TEXT, hashtags TEXT, attempts INTEGER DEFAULT 0, quota_deferrals INTEGER DEFAULT 0,
            created_at TIMESTAMPTZ DEFAULT NOW(), completed_at TIMESTAMPTZ, last_error TEXT, last_attempt_at TIMESTAMPTZ);
        CREATE TABLE social_media_streamer_settings (twitch_user_id TEXT, approval_mode TEXT DEFAULT 'manual',
            timezone TEXT DEFAULT 'UTC', subtitles_enabled BOOLEAN DEFAULT TRUE);
        CREATE TABLE social_media_platform_schedule (twitch_user_id TEXT, platform TEXT, auto_post BOOLEAN DEFAULT TRUE,
            posts_per_week INTEGER DEFAULT 7, max_posts_per_day INTEGER DEFAULT 1, post_times JSONB DEFAULT '[\"18:00\"]');
        INSERT INTO social_media_streamer_settings (twitch_user_id) VALUES ('owner');
        INSERT INTO social_media_platform_schedule (twitch_user_id, platform) VALUES ('owner', 'youtube'), ('owner', 'tiktok')")
        .execute(&pool).await.expect("legacy fixture schema");
    sqlx::raw_sql(include_str!(
        "../../../migrations/20261011100000_social_autopilot_contracts.sql"
    ))
    .execute(&pool)
    .await
    .expect("actual M0 migration");
    pool
}

async fn clip(pool: &PgPool, name: &str, owner: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, twitch_user_id)
        VALUES ($1, 'https://clips.test/fixture', $2, $2) RETURNING id::bigint",
    )
    .bind(name)
    .bind(owner)
    .fetch_one(pool)
    .await
    .expect("source fixture")
}

async fn expired_files(
    pool: &PgPool,
    dir: &std::path::Path,
    name: &str,
    owner: &str,
) -> (i64, Vec<PathBuf>) {
    let id = clip(pool, name, owner).await;
    let source = dir.join(format!("{name}-source.mp4"));
    let upload = dir.join(format!("{name}-upload.mp4"));
    let preview = dir.join(format!("{name}-preview.mp4"));
    let mut files = vec![source.clone(), upload.clone(), preview.clone()];
    for input in [&source, &upload] {
        for platform in crate::posting_plan::PLATFORMS {
            files.push(
                crate::upload_worker::vertical_output_path(&input.to_string_lossy(), platform)
                    .into(),
            );
        }
    }
    for file in &files {
        tokio::fs::write(file, b"retention fixture")
            .await
            .expect("real temporary file");
    }
    sqlx::query("UPDATE twitch_clips_social_media SET local_file_path = $2, upload_local_path = $3, preview_path = $4,
        retention_until = NOW() - INTERVAL '1 day', discarded_at = NOW() WHERE id = $1")
        .bind(id).bind(source.to_str()).bind(upload.to_str()).bind(preview.to_str()).execute(pool).await.expect("expired source");
    (id, files)
}

async fn wait_for_lock(pool: &PgPool, label: &str, kind: &str) {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks l JOIN pg_stat_activity a ON a.pid = l.pid
                WHERE a.datname = current_database() AND a.application_name = $1 AND l.locktype = $2 AND NOT l.granted)")
                .bind(label).bind(kind).fetch_one(pool).await.expect("real PostgreSQL lock observation");
            if waiting { break; }
            tokio::task::yield_now().await;
        }
    }).await.expect("operation reached the deterministic lock boundary");
}

#[tokio::test]
async fn retention_preserves_all_production_history_and_cleans_unrelated_sources() {
    let pool = fixture("t_m0_fix_retention_mixed").await;
    let dir = tempfile::tempdir().expect("temporary sources");
    let mut protected = Vec::new();
    for (order, status) in [
        "queued",
        "running",
        "review",
        "completed",
        "failed",
        "cancelled",
    ]
    .into_iter()
    .enumerate()
    {
        let item = expired_files(&pool, dir.path(), status, "owner").await;
        sqlx::query("INSERT INTO social_production_jobs (twitch_user_id, clip_id, queue_order, status, mode, started_at)
            VALUES ('owner', $1, $2, $3, 'static', NOW())")
            .bind(item.0).bind(order as i64).bind(status).execute(&pool).await.expect("job history");
        protected.push(item);
    }
    for status in [
        "rendering",
        "review",
        "approved",
        "scheduled",
        "published",
        "expired",
        "rejected",
        "failed",
    ] {
        let item = expired_files(&pool, dir.path(), &format!("video-{status}"), "owner").await;
        sqlx::query("INSERT INTO social_produced_videos (twitch_user_id, clip_id, mode, status, file_path, duration_ms, scheduled_at)
            VALUES ('owner', $1, 'static', $2, $3, 15000, CASE WHEN $2 = 'scheduled' THEN NOW() ELSE NULL END)")
            .bind(item.0).bind(status).bind(item.1[0].to_str()).execute(&pool).await.expect("video history");
        protected.push(item);
    }
    let unrelated = expired_files(&pool, dir.path(), "unrelated", "other").await;
    RetentionWorker::new(pool.clone()).run_once().await;
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM twitch_clips_social_media ORDER BY id")
        .fetch_all(&pool)
        .await
        .expect("retained rows");
    assert_eq!(ids, protected.iter().map(|item| item.0).collect::<Vec<_>>());
    assert!(protected
        .iter()
        .flat_map(|item| &item.1)
        .all(|file| file.is_file()));
    assert!(unrelated.1.iter().all(|file| !file.exists()));
    let counts: (i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM social_production_jobs), (SELECT count(*) FROM social_produced_videos)")
        .fetch_one(&pool).await.expect("preserved history");
    assert_eq!(counts, (6, 8));
    let additional = expired_files(&pool, dir.path(), "direct-delete", "other").await;
    let mut mixed = ids.clone();
    mixed.push(additional.0);
    retention::delete_clips_by_ids(&pool, &mixed).await;
    let remaining: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM twitch_clips_social_media ORDER BY id")
            .fetch_all(&pool)
            .await
            .expect("guarded direct deletion");
    assert_eq!(remaining, ids);
    pool.close().await;
}

#[tokio::test]
async fn retention_rechecks_a_reference_committed_while_source_lock_waits() {
    let schema = "t_m0_fix_reference_first";
    let pool = fixture(schema).await;
    let dir = tempfile::tempdir().expect("temporary source");
    let (id, files) = expired_files(&pool, dir.path(), "insert-first", "owner").await;
    let worker_pool = connection_pool(schema, "m0_fix_retention_waiter").await;
    let mut inserting = pool.begin().await.expect("reference transaction");
    sqlx::query("INSERT INTO social_production_jobs (twitch_user_id, clip_id, queue_order, status) VALUES ('owner', $1, 1, 'completed')")
        .bind(id).execute(&mut *inserting).await.expect("uncommitted FK reference");
    let worker = tokio::spawn(async move {
        RetentionWorker::new(worker_pool).run_once().await;
    });
    wait_for_lock(&pool, "m0_fix_retention_waiter", "transactionid").await;
    assert!(files.iter().all(|file| file.is_file()));
    inserting
        .commit()
        .await
        .expect("reference wins before source lock");
    worker.await.expect("retention completion");
    assert!(files.iter().all(|file| file.is_file()));
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM twitch_clips_social_media WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("protected source");
    assert_eq!(count, 1);
    pool.close().await;
}

#[tokio::test]
async fn retention_source_lock_blocks_new_fk_until_unlink_and_delete_commit() {
    let schema = "t_m0_fix_retention_first";
    let pool = fixture(schema).await;
    let dir = tempfile::tempdir().expect("temporary source");
    let (id, files) = expired_files(&pool, dir.path(), "delete-first", "owner").await;
    sqlx::query("UPDATE twitch_clips_social_media SET discarded_at = NULL WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("publication eligibility path");
    let worker_pool = connection_pool(schema, "m0_fix_retention_holder").await;
    let reference_pool = connection_pool(schema, "m0_fix_reference_waiter").await;
    let mut eligibility = pool.begin().await.expect("eligibility barrier");
    sqlx::query("LOCK TABLE social_media_platform_auth IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *eligibility)
        .await
        .expect("block publication check after source lock");
    let worker = tokio::spawn(async move {
        RetentionWorker::new(worker_pool).run_once().await;
    });
    wait_for_lock(&pool, "m0_fix_retention_holder", "relation").await;
    let insert = tokio::spawn(async move {
        sqlx::query("INSERT INTO social_production_jobs (twitch_user_id, clip_id, queue_order) VALUES ('owner', $1, 1)")
            .bind(id).execute(&reference_pool).await
    });
    wait_for_lock(&pool, "m0_fix_reference_waiter", "transactionid").await;
    assert!(files.iter().all(|file| file.is_file()));
    eligibility
        .commit()
        .await
        .expect("release eligibility barrier");
    worker.await.expect("real worker unlink and deletion");
    let error = insert
        .await
        .expect("reference insert completion")
        .expect_err("deleted source cannot gain history");
    assert!(error
        .as_database_error()
        .is_some_and(|error| error.is_foreign_key_violation()));
    assert!(files.iter().all(|file| !file.exists()));
    let counts: (i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM twitch_clips_social_media), (SELECT count(*) FROM social_production_jobs)")
        .fetch_one(&pool).await.expect("atomic deletion boundary");
    assert_eq!(counts, (0, 0));
    pool.close().await;
}

async fn ready_video(pool: &PgPool, file: &str, clip_id: i64) -> i64 {
    let video = produced_video::insert(
        pool,
        &NewProducedVideo {
            twitch_user_id: "owner".into(),
            clip_id,
            job_id: None,
            mode: ProductionMode::Static,
            version_no: 1,
            parent_version_id: None,
            variant_index: 0,
        },
    )
    .await
    .expect("rendering version");
    produced_video::complete_render(
        pool,
        "owner",
        video.id,
        &RenderedVideo {
            file_path: file.into(),
            duration_ms: 15000,
            needs_review: true,
            review_reasons: Vec::new(),
            metadata: json!({}),
        },
    )
    .await
    .expect("real produced file");
    produced_video::approve(pool, "owner", video.id, &["tiktok".into()])
        .await
        .expect("version approval");
    video.id
}

#[tokio::test]
async fn produced_decisions_use_database_time_after_owner_lock_waits() {
    for (schema, approve, past_slot) in [
        ("t_m0_time_approve", true, false),
        ("t_m0_time_enqueue_review", false, false),
        ("t_m0_time_enqueue_slot", false, true),
    ] {
        let pool = fixture(schema).await;
        let clip_id = clip(&pool, "decision-time", "owner").await;
        let dir = tempfile::tempdir().expect("render fixture");
        let file = dir.path().join("produced.mp4");
        tokio::fs::write(&file, b"render")
            .await
            .expect("render file");
        let id = ready_video(&pool, file.to_str().expect("file path"), clip_id).await;
        if !past_slot {
            sqlx::query("UPDATE social_produced_videos SET status = 'review', needs_review = TRUE WHERE id = $1")
                .bind(id).execute(&pool).await.expect("review state before lock wait");
        }
        if approve {
            sqlx::query("DELETE FROM social_media_clip_approval WHERE produced_video_id = $1")
                .bind(id)
                .execute(&pool)
                .await
                .expect("unapproved review");
        }
        let caller_pool = connection_pool(schema, schema).await;
        let mut barrier = pool.begin().await.expect("owner barrier");
        autopilot_settings::lock_owner(&mut barrier, "owner")
            .await
            .expect("hold owner lock");
        let caller = tokio::spawn(async move {
            let result = if approve {
                produced_video::approve(&caller_pool, "owner", id, &["tiktok".into()]).await
            } else {
                produced_video::enqueue_for_publishing_for_user(
                    &caller_pool,
                    "owner",
                    id,
                    &["tiktok".into()],
                )
                .await
                .map(|_| ())
            };
            caller_pool.close().await;
            result
        });
        wait_for_lock(&pool, schema, "advisory").await;
        let (started_at, deadline, observed_at): (DateTime<Utc>, DateTime<Utc>, DateTime<Utc>) =
            sqlx::query_as(
                "WITH observation AS MATERIALIZED (
                    SELECT a.xact_start AS started_at, clock_timestamp() AS observed_at
                    FROM pg_stat_activity a
                    WHERE a.datname = current_database() AND a.application_name = $1
                    AND EXISTS (SELECT 1 FROM pg_locks l WHERE l.pid = a.pid
                        AND l.locktype = 'advisory' AND NOT l.granted))
                SELECT started_at, started_at + (observed_at - started_at) / 2, observed_at
                FROM observation",
            )
            .bind(schema)
            .fetch_one(&pool)
            .await
            .expect("actual blocked transaction clock boundary");
        assert!(started_at < deadline && deadline < observed_at);
        if past_slot {
            sqlx::query("UPDATE social_produced_videos SET scheduled_at = $2 WHERE id = $1")
                .bind(id)
                .bind(deadline)
                .execute(&mut *barrier)
                .await
                .expect("reservation that elapsed during the owner lock wait");
        } else {
            sqlx::query("UPDATE social_produced_videos SET expires_at = $2 WHERE id = $1")
                .bind(id)
                .bind(deadline)
                .execute(&mut *barrier)
                .await
                .expect("review deadline crossed while blocked");
        }
        barrier.commit().await.expect("release owner lock");
        assert!(matches!(
            caller.await.expect("blocked decision completed"),
            Err(produced_video::ProducedVideoError::State)
        ));
        let persisted: (String, bool, i64, i64) = sqlx::query_as(
            "SELECT v.status, v.needs_review,
                (SELECT count(*) FROM social_media_clip_approval WHERE produced_video_id = v.id),
                (SELECT count(*) FROM twitch_clips_upload_queue WHERE produced_video_id = v.id)
            FROM social_produced_videos v WHERE v.id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("rejected decision left approval and queue unchanged");
        assert_eq!(
            persisted,
            (
                if past_slot { "approved" } else { "review" }.into(),
                !past_slot,
                i64::from(!approve),
                0,
            )
        );
        eprintln!("M0_DECISION_TIME case={schema} transaction_start={started_at} deadline={deadline} observed_at={observed_at} rejected=true persisted_unchanged=true");
        if past_slot {
            let existing: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_upload_queue (clip_id, produced_video_id, platform, status, scheduled_at) VALUES ($1, $2, 'tiktok', 'waiting_exact_file', $3) RETURNING id")
                .bind(clip_id).bind(id).bind(deadline).fetch_one(&pool).await.expect("existing exact-file reservation");
            let reservation = produced_video::enqueue_for_publishing_for_user(
                &pool,
                "owner",
                id,
                &["tiktok".into()],
            )
            .await
            .expect("intentional reuse of existing past reservation");
            assert_eq!(reservation.scheduled_at, deadline);
            assert_eq!(reservation.queue_ids, vec![("tiktok".into(), existing)]);
            let queue: Vec<(i64, String)> = sqlx::query_as(
                "SELECT id, status FROM twitch_clips_upload_queue WHERE produced_video_id = $1",
            )
            .bind(id)
            .fetch_all(&pool)
            .await
            .expect("unchanged exact-file queue identity");
            assert_eq!(queue, vec![(existing, "waiting_exact_file".into())]);
            assert!(file.is_file());
        }
        pool.close().await;
    }
}

#[tokio::test]
async fn legacy_and_produced_reservations_share_owner_lock_in_both_orders() {
    for (schema, produced_first, legacy_settings) in [
        ("t_m0_fix_produced_first", true, true),
        ("t_m0_fix_legacy_first", false, true),
        ("t_m0_fix_produced_first_no_settings", true, false),
        ("t_m0_fix_legacy_first_no_settings", false, false),
    ] {
        let pool = fixture(schema).await;
        if !legacy_settings {
            sqlx::query("DELETE FROM social_media_streamer_settings")
                .execute(&pool)
                .await
                .expect("owner without legacy settings");
        }
        autopilot_settings::save(
            &pool,
            "owner",
            &AutopilotSettings {
                timezone: if legacy_settings {
                    "UTC"
                } else {
                    "Europe/Berlin"
                }
                .into(),
                schedule_slots: (1..=7)
                    .map(|weekday| ScheduleSlot {
                        weekday,
                        local_time: "18:00".into(),
                    })
                    .collect(),
                ..AutopilotSettings::default()
            },
        )
        .await
        .expect("shared daily slots");
        let legacy_id = clip(&pool, "legacy", "owner").await;
        sqlx::query("INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms) VALUES ($1, 'approved', '[\"youtube\"]')")
            .bind(legacy_id as i32).execute(&pool).await.expect("legacy approval");
        let produced_clip = clip(&pool, "produced", "owner").await;
        let dir = tempfile::tempdir().expect("render fixture");
        let file = dir.path().join("produced.mp4");
        tokio::fs::write(&file, b"render")
            .await
            .expect("render file");
        let video = ready_video(&pool, file.to_str().expect("file path"), produced_clip).await;
        let legacy_pool = connection_pool(schema, "m0_fix_legacy_reservation").await;
        let produced_pool = connection_pool(schema, "m0_fix_produced_reservation").await;
        let mut barrier = pool.begin().await.expect("owner barrier");
        autopilot_settings::lock_owner(&mut barrier, "owner")
            .await
            .expect("owner transaction lock");
        let start_legacy = || {
            tokio::spawn(async move {
                approval::ensure_queued_uploads(&legacy_pool, legacy_id as i32).await
            })
        };
        let start_produced = || {
            tokio::spawn(async move {
                produced_video::enqueue_for_publishing(&produced_pool, video, &["tiktok".into()])
                    .await
            })
        };
        let (legacy, produced) = if produced_first {
            let produced = start_produced();
            wait_for_lock(&pool, "m0_fix_produced_reservation", "advisory").await;
            let legacy = start_legacy();
            wait_for_lock(&pool, "m0_fix_legacy_reservation", "advisory").await;
            (legacy, produced)
        } else {
            let legacy = start_legacy();
            wait_for_lock(&pool, "m0_fix_legacy_reservation", "advisory").await;
            let produced = start_produced();
            wait_for_lock(&pool, "m0_fix_produced_reservation", "advisory").await;
            (legacy, produced)
        };
        barrier.commit().await.expect("release ordered contenders");
        let legacy = legacy.await.expect("legacy operation");
        assert_eq!(legacy.len(), 1);
        let produced = produced
            .await
            .expect("produced operation")
            .expect("produced reservation");
        let legacy_slot: DateTime<Utc> =
            sqlx::query_scalar("SELECT scheduled_at FROM twitch_clips_upload_queue WHERE id = $1")
                .bind(legacy[0].1)
                .fetch_one(&pool)
                .await
                .expect("legacy slot");
        assert_ne!(legacy_slot, produced.scheduled_at);
        assert_eq!(produced_first, produced.scheduled_at < legacy_slot);
        assert_eq!(
            approval::ensure_queued_uploads(&pool, legacy_id as i32)
                .await
                .len(),
            0
        );
        pool.close().await;
    }
}

#[tokio::test]
async fn legacy_reservations_lock_and_deduplicate_without_any_settings_row() {
    let schema = "t_m0_fix_no_settings";
    let pool = fixture(schema).await;
    sqlx::raw_sql(
        "DELETE FROM social_media_streamer_settings; DELETE FROM social_media_platform_schedule",
    )
    .execute(&pool)
    .await
    .expect("owner without settings rows");
    let id = clip(&pool, "no-settings", "owner").await;
    sqlx::query("INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms) VALUES ($1, 'approved', '[\"youtube\"]')")
        .bind(id as i32).execute(&pool).await.expect("legacy approval");
    let first_pool = connection_pool(schema, "m0_fix_first_no_settings").await;
    let second_pool = connection_pool(schema, "m0_fix_second_no_settings").await;
    let mut barrier = pool.begin().await.expect("owner barrier");
    autopilot_settings::lock_owner(&mut barrier, "owner")
        .await
        .expect("owner lock without settings");
    let first =
        tokio::spawn(async move { approval::ensure_queued_uploads(&first_pool, id as i32).await });
    wait_for_lock(&pool, "m0_fix_first_no_settings", "advisory").await;
    let second =
        tokio::spawn(async move { approval::ensure_queued_uploads(&second_pool, id as i32).await });
    wait_for_lock(&pool, "m0_fix_second_no_settings", "advisory").await;
    barrier.commit().await.expect("release ordered contenders");
    assert_eq!(first.await.expect("first reservation").len(), 1);
    assert!(second.await.expect("deduplicated reservation").is_empty());
    let counts: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT
        (SELECT count(*) FROM twitch_clips_upload_queue WHERE scheduled_at IS NOT NULL),
        (SELECT count(*) FROM social_media_streamer_settings),
        (SELECT count(*) FROM social_media_platform_schedule),
        (SELECT count(*) FROM social_media_autopilot_settings)",
    )
    .fetch_one(&pool)
    .await
    .expect("reservation without settings creation");
    assert_eq!(counts, (1, 0, 0, 0));
    pool.close().await;
}

#[tokio::test]
async fn complete_migration_chain_keeps_non_owner_approval_writers_operational() {
    let database = format!("m0_fix_roles_{}", std::process::id());
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options())
        .await
        .expect("isolated admin");
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {database}")))
        .execute(&admin)
        .await
        .expect("own empty database");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options().database(&database))
        .await
        .expect("fresh database");
    sqlx::raw_sql("DO $$ DECLARE runtime_role TEXT; BEGIN
        FOREACH runtime_role IN ARRAY ARRAY['twitchbot', 'twitchdash', 'twitchlegacy'] LOOP
            IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = runtime_role) THEN
                EXECUTE format('CREATE ROLE %I LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS', runtime_role);
            END IF;
        END LOOP; END $$;
        CREATE EXTENSION IF NOT EXISTS timescaledb")
        .execute(&pool).await.expect("non-owner runtime roles and actual engine");
    let mut legacy = sqlx::migrate!("../../migrations");
    legacy.migrations = Cow::Owned(
        legacy
            .iter()
            .filter(|migration| migration.version < M0)
            .cloned()
            .collect(),
    );
    legacy
        .run(&pool)
        .await
        .expect("complete pre-M0 migration chain");
    sqlx::raw_sql("GRANT SELECT, INSERT, UPDATE, DELETE ON social_media_clip_approval, twitch_clips_social_media, twitch_clips_upload_queue TO twitchbot, twitchdash, twitchlegacy")
        .execute(&pool).await.expect("existing legacy table permissions only");
    let default_grants: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pg_default_acl WHERE defaclobjtype = 'S'")
            .fetch_one(&pool)
            .await
            .expect("sequence defaults");
    assert_eq!(default_grants, 0);
    let ids: Vec<i64> = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, created_at)
        SELECT 'role-proof-' || g, 'https://clips.test/role-proof', 'fixture', NOW() FROM generate_series(1, 3) g RETURNING id")
        .fetch_all(&pool).await.expect("legacy source fixtures before migration");
    MIGRATOR
        .run(&pool)
        .await
        .expect("M0 transaction commits within complete chain");
    let applied: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success")
        .fetch_one(&pool)
        .await
        .expect("all migrations committed");
    assert_eq!(applied as usize, MIGRATOR.iter().count());
    for (role, id) in ["twitchbot", "twitchdash", "twitchlegacy"]
        .into_iter()
        .zip(ids)
    {
        let runtime = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(options().database(&database).username(role))
            .await
            .expect("actual runtime-role connection");
        let identity: (String, bool, bool, bool) = sqlx::query_as("SELECT current_user::text, r.rolsuper,
            c.relowner = r.oid, has_sequence_privilege(current_user, 'social_media_clip_approval_approval_id_seq', 'USAGE')
            FROM pg_roles r CROSS JOIN pg_class c WHERE r.rolname = current_user AND c.oid = 'social_media_clip_approval'::regclass")
            .fetch_one(&runtime).await.expect("actual non-owner permissions");
        assert_eq!(identity, (role.into(), false, false, true));
        let broad_sequence_privileges: bool = sqlx::query_scalar("SELECT
            has_sequence_privilege(current_user, 'social_media_clip_approval_approval_id_seq', 'SELECT') OR
            has_sequence_privilege(current_user, 'social_media_clip_approval_approval_id_seq', 'UPDATE')")
            .fetch_one(&runtime).await.expect("narrow sequence grant");
        assert!(!broad_sequence_privileges);
        let record = approval::ensure_approval_row(&runtime, id as i32).await;
        assert_eq!(record.state, approval::STATE_AWAITING);
        let persisted: i64 = sqlx::query_scalar(
            "SELECT approval_id FROM social_media_clip_approval WHERE clip_db_id = $1",
        )
        .bind(id as i32)
        .fetch_one(&runtime)
        .await
        .expect("legacy INSERT persisted using new sequence");
        assert!(persisted > 0);
        sqlx::query("UPDATE social_media_clip_approval SET state = 'approved', approved_platforms = '[\"youtube\"]' WHERE clip_db_id = $1")
            .bind(id as i32).execute(&runtime).await.expect("approved legacy state");
        let outcome = approval::cancel_scheduled_uploads(&runtime, id as i32)
            .await
            .expect("legacy INSERT ON CONFLICT cancellation");
        assert_eq!(outcome.cancelled, 0);
        let state: String = sqlx::query_scalar(
            "SELECT state FROM social_media_clip_approval WHERE clip_db_id = $1",
        )
        .bind(id as i32)
        .fetch_one(&runtime)
        .await
        .expect("cancel state committed");
        assert_eq!(state, approval::STATE_AWAITING);
        eprintln!("M0_FULL_CHAIN role={role} non_owner=true insert=true cancellation_conflict=true sequence_usage_only=true");
        runtime.close().await;
    }
    eprintln!("M0_FULL_CHAIN migrations={applied} role_refresh=false");
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP DATABASE {database}")))
        .execute(&admin)
        .await
        .expect("remove own permission fixture database");
    admin.close().await;
}
