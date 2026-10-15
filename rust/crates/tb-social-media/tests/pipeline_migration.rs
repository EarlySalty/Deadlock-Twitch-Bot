#[path = "../../../test-support/postgres.rs"]
mod postgres;

use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tb_social_media::pipeline_migration::{self, APPROVALS, QUEUE, SCHEDULES};

async fn database() -> (postgres::TestPostgres, PgPool) {
    let db = postgres::TestPostgres::start_with_timescaledb().await;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with((*db.pool.connect_options()).clone())
        .await
        .expect("Pool");
    sqlx::query("CREATE DATABASE twitch_analytics")
        .execute(&pool)
        .await
        .expect("Analytics-Datenbank");
    sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb")
        .execute(&pool)
        .await
        .expect("TimescaleDB");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("Migrationen");
    (db, pool)
}

async fn seed(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('alpha', '100'), ('beta', '200');
        INSERT INTO social_media_streamer_settings (streamer_login, approval_mode, timezone, twitch_user_id)
            VALUES ('alpha', 'veto_window', 'Europe/Berlin', '100');
        INSERT INTO social_media_platform_schedule (streamer_login, platform, auto_post, posts_per_week, max_posts_per_day, post_times, twitch_user_id) VALUES
            ('alpha', 'youtube', TRUE, 5, 1, '["18:00"]', '100'),
            ('alpha', 'tiktok', TRUE, 5, 1, '["18:00"]', '100'),
            ('alpha', 'instagram', FALSE, 0, 0, '[]', '100'),
            ('beta', 'youtube', TRUE, 3, 1, '["20:00"]', NULL);
        INSERT INTO social_media_category_settings (streamer_login, category_key, auto_post, twitch_user_id) VALUES
            ('alpha', 'deadlock', TRUE, '100'), ('alpha', 'other', TRUE, '100');
        INSERT INTO social_media_autopilot_settings (twitch_user_id, enabled, timezone, schedule_slots, target_platforms)
            VALUES ('200', TRUE, 'UTC', '[{"weekday":1,"local_time":"09:00"}]', '["youtube"]');
        INSERT INTO twitch_clips_social_media (id, clip_id, clip_url, streamer_login, twitch_user_id, category_key, custom_title, hashtags, youtube_video_id, uploaded_youtube) VALUES
            (1, 'c1', 'https://clips/c1', 'alpha', '100', 'deadlock', 'Eigener Titel', '#deadlock #clip', NULL, NULL),
            (2, 'c2', 'https://clips/c2', 'alpha', '100', 'deadlock', NULL, NULL, NULL, NULL),
            (3, 'c3', 'https://clips/c3', 'alpha', '100', 'deadlock', NULL, NULL, NULL, NULL),
            (4, 'c4', 'https://clips/c4', 'alpha', '100', 'other', NULL, NULL, NULL, NULL),
            (5, 'c5', 'https://clips/c5', 'alpha', '100', 'deadlock', NULL, NULL, 'yt-5', TRUE),
            (6, 'c6', 'https://clips/c6', 'alpha', '100', 'deadlock', NULL, NULL, NULL, NULL),
            (7, 'c7', 'https://clips/c7', 'ghost', NULL, 'deadlock', NULL, NULL, NULL, NULL);
        INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms, approver_user_id, decided_at) VALUES
            (1, 'awaiting_approval', '["youtube","tiktok"]', NULL, NULL),
            (2, 'approved', '["tiktok"]', 'mod-1', '2026-10-01T10:00:00Z'),
            (3, 'skipped', '[]', 'mod-1', '2026-10-02T10:00:00Z'),
            (4, 'awaiting_approval', '["youtube"]', NULL, NULL),
            (7, 'awaiting_approval', '["youtube"]', NULL, NULL);
        INSERT INTO social_media_clip_enrichment (clip_db_id, title_youtube, title_tiktok, hashtags_tiktok, status)
            VALUES (1, 'KI-Titel', 'KI-Titel TikTok', '["deadlock"]', 'done');
        INSERT INTO twitch_clips_upload_queue (clip_id, platform, status, title, description, hashtags, scheduled_at) VALUES
            (1, 'youtube', 'pending', 'Warteschlangentitel', 'Beschreibung', '#deadlock', '2026-11-01T18:00:00Z'),
            (2, 'tiktok', 'waiting_schedule', 'Freigegeben', NULL, NULL, '2026-11-02T18:00:00Z'),
            (5, 'youtube', 'completed', 'Fertig', NULL, NULL, NULL),
            (6, 'youtube', 'processing', 'Läuft', NULL, NULL, NULL);
        INSERT INTO social_media_settings (key, value) VALUES
            ('tiktok_draft:100:1', '{"caption":"Mein Entwurf","privacy_level":"SELF_ONLY","allow_comment":true,"allow_duet":false,"allow_stitch":false,"music_consent":true,"music_consent_at":"2026-10-01T10:00:00Z","music_consent_platform_user_id":"tt-1"}'),
            ('tiktok_draft:100:999', '{"caption":"Verwaist","privacy_level":"SELF_ONLY","allow_comment":true,"allow_duet":true,"allow_stitch":true}'),
            ('tiktok_defaults:100', '{"caption":"","standard_description":"Folgt mir","privacy_level":"PUBLIC_TO_EVERYONE","allow_comment":true,"allow_duet":true,"allow_stitch":true}');
        "#,
    )
    .execute(pool)
    .await
    .expect("Testdaten");
}

async fn legacy_fingerprint(pool: &PgPool) -> String {
    let mut parts = Vec::new();
    for table in [
        "twitch_clips_social_media",
        "twitch_clips_upload_queue",
        "social_media_clip_approval",
        "social_media_clip_enrichment",
        "social_media_streamer_settings",
        "social_media_platform_schedule",
        "social_media_category_settings",
        "social_media_settings",
        "social_media_autopilot_settings",
    ] {
        let rows: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COALESCE(string_agg(to_jsonb(t)::text, '|' ORDER BY to_jsonb(t)::text), '') FROM {table} t"
        )))
        .fetch_one(pool)
        .await
        .expect("Fingerabdruck");
        parts.push(rows);
    }
    parts.join("\n")
}

async fn map_fingerprint(pool: &PgPool) -> String {
    sqlx::query_scalar(
        "SELECT COALESCE(string_agg(to_jsonb(m)::text, '|' ORDER BY twitch_user_id, legacy_table, legacy_key), '')
         FROM social_pipeline_migration_map m",
    )
    .fetch_one(pool)
    .await
    .expect("Abbildung")
}

async fn mapping(pool: &PgPool, table: &str, key: &str) -> (String, Option<String>, Value) {
    let (disposition, reason, subject): (String, Option<String>, String) = sqlx::query_as(
        "SELECT disposition, unresolved_reason, COALESCE(canonical_subject, '{}')::text
         FROM social_pipeline_migration_map WHERE legacy_table = $1 AND legacy_key = $2",
    )
    .bind(table)
    .bind(key)
    .fetch_one(pool)
    .await
    .expect("Abbildungszeile");
    (
        disposition,
        reason,
        serde_json::from_str(&subject).expect("JSON"),
    )
}

async fn approval_key(pool: &PgPool, clip: i64) -> String {
    sqlx::query_scalar(
        "SELECT approval_id::text FROM social_media_clip_approval WHERE clip_db_id = $1",
    )
    .bind(clip)
    .fetch_one(pool)
    .await
    .expect("Freigabe")
}

async fn queue_key(pool: &PgPool, clip: i64) -> String {
    sqlx::query_scalar("SELECT id::text FROM twitch_clips_upload_queue WHERE clip_id = $1")
        .bind(clip)
        .fetch_one(pool)
        .await
        .expect("Warteschlange")
}

#[tokio::test]
async fn legacy_zustand_wird_verlustfrei_und_idempotent_abgebildet() {
    let (_db, pool) = database().await;
    seed(&pool).await;
    let legacy_before = legacy_fingerprint(&pool).await;

    let dry = pipeline_migration::dry_run(&pool).await.expect("Probelauf");
    assert!(!dry.applied);
    assert_eq!(map_fingerprint(&pool).await, "");
    let cutovers: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM social_pipeline_cutover")
        .fetch_one(&pool)
        .await
        .expect("Umstellung");
    assert_eq!(cutovers, 0);
    assert_eq!(dry.legacy_awaiting, 3);
    assert_eq!(dry.preserved_awaiting, 2);
    assert_eq!(dry.legacy_approved, 1);
    assert_eq!(dry.preserved_approved, 1);
    assert_eq!(dry.orphans.get(APPROVALS), Some(&1));
    assert_eq!(dry.inserted, dry.rows);

    let first = pipeline_migration::apply(&pool).await.expect("Anwendung");
    assert_eq!(first.rows, dry.rows);
    assert_eq!(first.totals, dry.totals);
    assert_eq!(first.cutover_changed, 2);
    assert_eq!(legacy_fingerprint(&pool).await, legacy_before);
    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM social_pipeline_migration_map")
        .fetch_one(&pool)
        .await
        .expect("Zeilen");
    assert_eq!(stored as usize, first.rows);

    let (disposition, _, subject) = mapping(&pool, APPROVALS, &approval_key(&pool, 1).await).await;
    assert_eq!(disposition, "mapped");
    assert_eq!(subject["decision"], "pending");
    assert_eq!(subject["publishable"], false);
    assert_eq!(
        subject["selected_platforms"],
        serde_json::json!(["youtube", "tiktok"])
    );
    let (_, _, subject) = mapping(&pool, APPROVALS, &approval_key(&pool, 4).await).await;
    assert_eq!(subject["eligibility"], "legacy_non_deadlock_visible");
    let (disposition, _, subject) = mapping(&pool, APPROVALS, &approval_key(&pool, 2).await).await;
    assert_eq!(disposition, "mapped");
    assert_eq!(subject["decision"], "approved_legacy");
    assert_eq!(subject["approver_user_id"], "mod-1");
    assert_eq!(subject["requires_exact_version_approval"], true);
    let (disposition, _, _) = mapping(&pool, APPROVALS, &approval_key(&pool, 3).await).await;
    assert_eq!(disposition, "history");

    let (disposition, reason, _) = mapping(&pool, QUEUE, &queue_key(&pool, 6).await).await;
    assert_eq!(disposition, "unresolved");
    assert_eq!(reason.as_deref(), Some("in_flight_claim"));
    let (disposition, _, subject) = mapping(&pool, QUEUE, &queue_key(&pool, 2).await).await;
    assert_eq!(disposition, "mapped");
    assert_eq!(subject["scheduled_at"], "2026-11-02T18:00:00+00:00");
    let (disposition, _, _) = mapping(&pool, QUEUE, &queue_key(&pool, 5).await).await;
    assert_eq!(disposition, "history");

    let (_, _, subject) = mapping(&pool, "twitch_clips_social_media", "1").await;
    let youtube = &subject["text"]["platforms"]["youtube"];
    assert_eq!(youtube["title"]["value"], "Warteschlangentitel");
    assert_eq!(youtube["title"]["origin"], "legacy_queue");
    assert_eq!(
        subject["text"]["platforms"]["tiktok"]["title"]["origin"],
        "enrichment"
    );
    assert_eq!(
        subject["text"]["platforms"]["instagram"]["title"]["origin"],
        "clip"
    );
    assert_eq!(subject["text"]["scope"], "per_platform");
    assert!(!subject["text_conflicts"]
        .as_array()
        .expect("Konflikte")
        .is_empty());
    let (_, _, subject) = mapping(&pool, "twitch_clips_social_media", "5").await;
    assert_eq!(subject["published"]["youtube"], "yt-5");

    let (disposition, _, subject) =
        mapping(&pool, "social_media_settings", "tiktok_draft:100:1").await;
    assert_eq!(disposition, "mapped");
    assert_eq!(subject["choices"]["caption"], "Mein Entwurf");
    assert_eq!(subject["music_consent"]["scope"], "account");
    assert_eq!(subject["music_consent"]["platform_user_id"], "tt-1");
    assert_eq!(subject["authorization"], false);
    let (_, reason, _) = mapping(&pool, "social_media_settings", "tiktok_draft:100:999").await;
    assert_eq!(reason.as_deref(), Some("draft_clip_missing"));
    let (disposition, _, subject) =
        mapping(&pool, "social_media_settings", "tiktok_defaults:100").await;
    assert_eq!(disposition, "mapped");
    assert_eq!(subject["text_default_override"]["description"], "Folgt mir");

    let (_, _, subject) = mapping(&pool, "social_media_streamer_settings", "alpha").await;
    assert_eq!(subject["review_policy"], "veto_until_deadline");
    let (disposition, _, subject) = mapping(&pool, SCHEDULES, "alpha:youtube").await;
    assert_eq!(disposition, "mapped");
    let weekdays: Vec<u64> = subject["slots"]
        .as_array()
        .expect("Slots")
        .iter()
        .filter_map(|slot| slot["weekday"].as_u64())
        .collect();
    assert_eq!(weekdays, vec![1, 2, 3, 5, 6]);
    let (disposition, reason, _) = mapping(&pool, SCHEDULES, "beta:youtube").await;
    assert_eq!(disposition, "unresolved");
    assert_eq!(reason.as_deref(), Some("conflicts_with_canonical_schedule"));
    let (disposition, _, _) = mapping(&pool, "social_media_category_settings", "alpha:other").await;
    assert_eq!(disposition, "history");

    let (state, reconciliation): (String, String) = sqlx::query_as(
        "SELECT state, reconciliation::text FROM social_pipeline_cutover WHERE twitch_user_id = '100'",
    )
    .fetch_one(&pool)
    .await
    .expect("Umstellung");
    let reconciliation: Value = serde_json::from_str(&reconciliation).expect("JSON");
    assert_eq!(state, "shadow");
    let proposal = &reconciliation["proposed_policy"];
    assert_eq!(proposal["policy"]["review_policy"], "veto_until_deadline");
    assert_eq!(proposal["policy"]["schedule_scope"], "shared");
    assert_eq!(
        proposal["target_platforms"],
        serde_json::json!(["tiktok", "youtube"])
    );
    assert_eq!(proposal["timezone"], "Europe/Berlin");
    assert_eq!(
        proposal["policy"]["text_defaults"]["per_platform"]["tiktok"]["description"],
        "Folgt mir"
    );
    assert!(first
        .conflicts
        .iter()
        .any(|conflict| conflict.reason == "in_flight_claim"));

    let map_before = map_fingerprint(&pool).await;
    let cutover_before: String = sqlx::query_scalar(
        "SELECT string_agg(to_jsonb(c)::text, '|' ORDER BY twitch_user_id) FROM social_pipeline_cutover c",
    )
    .fetch_one(&pool)
    .await
    .expect("Umstellung");
    let second = pipeline_migration::apply(&pool)
        .await
        .expect("Zweiter Lauf");
    assert_eq!(second.inserted, 0);
    assert_eq!(second.updated, 0);
    assert_eq!(second.unchanged, first.rows);
    assert_eq!(second.stale, 0);
    assert_eq!(second.cutover_changed, 0);
    assert_eq!(map_fingerprint(&pool).await, map_before);
    let cutover_after: String = sqlx::query_scalar(
        "SELECT string_agg(to_jsonb(c)::text, '|' ORDER BY twitch_user_id) FROM social_pipeline_cutover c",
    )
    .fetch_one(&pool)
    .await
    .expect("Umstellung");
    assert_eq!(cutover_after, cutover_before);
    assert_eq!(legacy_fingerprint(&pool).await, legacy_before);

    let again = pipeline_migration::dry_run(&pool)
        .await
        .expect("Probelauf danach");
    assert_eq!(again.inserted, 0);
    assert_eq!(again.updated, 0);
}

#[tokio::test]
async fn probelauf_ohne_altschema_meldet_fehler_statt_zu_schreiben() {
    let db = postgres::TestPostgres::start().await;
    assert!(pipeline_migration::dry_run(&db.pool).await.is_err());
}
