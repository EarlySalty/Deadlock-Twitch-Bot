#[path = "../../../test-support/postgres.rs"]
mod postgres;

use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tb_social_media::clip_queue;
use tb_social_media::pipeline_migration::{
    self, CutoverCommand, CutoverOutcome, APPROVALS, QUEUE, SCHEDULES,
};

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
            ('beta', 'youtube', TRUE, 3, 1, '["20:00"]', NULL),
            ('beta', 'tiktok', FALSE, 4, 1, '["18:00"]', NULL);
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
    let (disposition, reason, _) = mapping(&pool, SCHEDULES, "beta:tiktok").await;
    assert_eq!(disposition, "mapped");
    assert_eq!(reason, None);
    let beta: String = sqlx::query_scalar(
        "SELECT reconciliation::text FROM social_pipeline_cutover WHERE twitch_user_id = '200'",
    )
    .fetch_one(&pool)
    .await
    .expect("Umstellung beta");
    let beta: Value = serde_json::from_str(&beta).expect("JSON");
    let beta_proposal = &beta["proposed_policy"];
    assert_eq!(
        beta_proposal["target_platforms"],
        serde_json::json!(["youtube"])
    );
    assert_eq!(
        beta_proposal["schedule_slots"],
        serde_json::json!([{"weekday": 1, "local_time": "09:00"}])
    );
    assert_eq!(beta["ready"], false);
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

async fn seed_cutover(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('gamma', '300'), ('delta', '400'), ('epsilon', '500');
        INSERT INTO social_media_autopilot_settings (twitch_user_id, enabled, publish_mode, timezone)
            VALUES ('500', TRUE, 'review', 'Europe/Berlin');
        INSERT INTO social_media_streamer_settings (streamer_login, approval_mode, timezone, twitch_user_id)
            VALUES ('gamma', 'veto_window', 'Europe/Berlin', '300'),
            ('epsilon', 'full_auto', 'Europe/Berlin', '500');
        INSERT INTO social_media_platform_schedule (streamer_login, platform, auto_post, posts_per_week, max_posts_per_day, post_times, twitch_user_id) VALUES
            ('gamma', 'youtube', TRUE, 3, 1, '["18:00"]', '300'),
            ('gamma', 'tiktok', FALSE, 0, 0, '[]', '300');
        INSERT INTO twitch_clips_social_media (id, clip_id, clip_url, streamer_login, twitch_user_id, category_key) VALUES
            (20, 'c20', 'https://clips/c20', 'gamma', '300', 'deadlock'),
            (21, 'c21', 'https://clips/c21', 'gamma', NULL, 'deadlock'),
            (22, 'c22', 'https://clips/c22', 'gamma', '300', 'other'),
            (30, 'c30', 'https://clips/c30', 'delta', '400', 'deadlock'),
            (31, 'c31', 'https://clips/c31', 'delta', '400', 'deadlock');
        INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms) VALUES
            (21, 'awaiting_approval', '["youtube"]'),
            (22, 'awaiting_approval', '["youtube"]');
        INSERT INTO twitch_clips_upload_queue (clip_id, platform, status, title, scheduled_at) VALUES
            (20, 'youtube', 'pending', 'Zugesagt', '2026-01-01T18:00:00Z'),
            (30, 'youtube', 'processing', 'Läuft', NULL);
        "#,
    )
    .execute(pool)
    .await
    .expect("Umstellungsdaten");
}

#[tokio::test]
async fn gespeicherte_pruefregel_aus_planen_bleibt_bei_der_umstellung() {
    let (_db, pool) = database().await;
    seed_cutover(&pool).await;
    sqlx::query(
        "UPDATE social_media_autopilot_settings SET pipeline_mode = 'ai_edit', review_policy = 'required',
            policy_revision = 1 WHERE twitch_user_id = '500'",
    )
    .execute(&pool)
    .await
    .expect("Planen-Speicherung");
    let preview = pipeline_migration::dry_run(&pool).await.expect("Probelauf");
    assert_eq!(
        preview.channel_policies.get("500"),
        Some(&serde_json::json!({
            "review_policy": {"legacy": "off", "applied": "required", "source": "planen"},
            "pipeline_mode": {"applied": "ai_edit", "source": "planen"},
        }))
    );
    pipeline_migration::run_command(&pool, &CutoverCommand::Apply)
        .await
        .expect("Umstellung");
    let kept: (String, String) = sqlx::query_as(
        "SELECT review_policy, pipeline_mode FROM social_media_autopilot_settings WHERE twitch_user_id = '500'",
    )
    .fetch_one(&pool)
    .await
    .expect("Prüfregel");
    assert_eq!(kept, ("required".into(), "ai_edit".into()));
}

async fn legacy_counts(pool: &PgPool) -> (i64, i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM twitch_clips_upload_queue),
            (SELECT count(*) FROM social_media_clip_approval),
            (SELECT count(*) FROM twitch_clips_social_media)",
    )
    .fetch_one(pool)
    .await
    .expect("Zählung")
}

async fn cutover_state(pool: &PgPool, owner: &str) -> String {
    sqlx::query_scalar("SELECT state FROM social_pipeline_cutover WHERE twitch_user_id = $1")
        .bind(owner)
        .fetch_one(pool)
        .await
        .expect("Umstellungszustand")
}

async fn legacy_ids(pool: &PgPool) -> Vec<i64> {
    clip_queue::get_upload_queue(pool, None, "pending", 50, None)
        .await
        .into_iter()
        .map(|item| item.clip_db_id)
        .collect()
}

async fn autopilot_paused(pool: &PgPool, owner: &str) -> bool {
    sqlx::query_scalar(
        "SELECT paused FROM social_media_autopilot_settings WHERE twitch_user_id = $1",
    )
    .bind(owner)
    .fetch_one(pool)
    .await
    .expect("Pausenzustand")
}

fn rejection(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>) -> String {
    result
        .expect_err("Altschreiber muss abgewiesen werden")
        .to_string()
}

#[tokio::test]
async fn umstellung_schliesst_altschreiber_und_pausiert_ohne_nachholen() {
    let (_db, pool) = database().await;
    seed_cutover(&pool).await;
    sqlx::query(
        "UPDATE social_media_autopilot_settings SET pipeline_mode = 'ai_edit' WHERE twitch_user_id = '500'",
    )
    .execute(&pool)
    .await
    .expect("Voreinstellung");
    let before = legacy_counts(&pool).await;
    let preview = pipeline_migration::dry_run(&pool).await.expect("Probelauf");
    assert_eq!(
        preview.channel_policies.get("500"),
        Some(&serde_json::json!({
            "review_policy": {"legacy": "off", "applied": "off", "source": "legacy"},
            "pipeline_mode": {"applied": "upload", "source": "legacy"},
        }))
    );
    assert_eq!(
        preview.channel_policies["300"]["pipeline_mode"],
        serde_json::json!({"applied": "upload", "source": "legacy"})
    );

    let first = pipeline_migration::run_command(&pool, &CutoverCommand::Apply)
        .await
        .expect("Umstellung");
    assert_eq!(first["cutover"].as_array().map(Vec::len), Some(3));
    let epsilon = first["cutover"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["twitch_user_id"] == "500"))
        .expect("epsilon");
    assert_eq!(epsilon["outcome"], "activated");
    assert_eq!(epsilon["unresolved"], 0);
    let mapped: (String, String, String) = sqlx::query_as(
        "SELECT review_policy, publish_mode, pipeline_mode FROM social_media_autopilot_settings WHERE twitch_user_id = '500'",
    )
    .fetch_one(&pool)
    .await
    .expect("Prüfregel");
    assert_eq!(mapped, ("off".into(), "full_auto".into(), "upload".into()));
    let gamma = first["cutover"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["twitch_user_id"] == "300"))
        .expect("gamma");
    assert_eq!(gamma["outcome"], "activated");
    assert_eq!(gamma["autopilot_enabled"], true);
    let delta = first["cutover"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["twitch_user_id"] == "400"))
        .expect("delta");
    assert_eq!(delta["outcome"], "not_ready");
    assert_eq!(cutover_state(&pool, "300").await, "unified");
    assert_eq!(cutover_state(&pool, "400").await, "shadow");
    assert_eq!(legacy_counts(&pool).await, before);

    let (review, configured, targets, slots, timezone): (String, bool, String, String, String) =
        sqlx::query_as(
            "SELECT review_policy, pipeline_mode IS NOT NULL, target_platforms::text,
                schedule_slots::text, timezone
             FROM social_media_autopilot_settings WHERE twitch_user_id = '300'",
        )
        .fetch_one(&pool)
        .await
        .expect("Autopilot");
    assert_eq!(review, "veto_until_deadline");
    assert!(configured);
    assert_eq!(targets, r#"["youtube"]"#);
    let slots: Value = serde_json::from_str(&slots).expect("Slots");
    let weekdays: Vec<u64> = slots
        .as_array()
        .expect("Slots")
        .iter()
        .filter_map(|slot| slot["weekday"].as_u64())
        .collect();
    assert_eq!(weekdays.len(), 3);
    assert!(slots
        .as_array()
        .expect("Slots")
        .iter()
        .all(|slot| slot["local_time"] == "18:00"));
    assert_eq!(timezone, "Europe/Berlin");

    let closed = rejection(
        sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) VALUES (21, 'tiktok', 'pending')")
            .execute(&pool)
            .await,
    );
    assert!(closed.contains("nur noch über den Autopiloten"), "{closed}");
    let approval = rejection(
        sqlx::query(
            "UPDATE social_media_clip_approval SET state = 'approved' WHERE clip_db_id = 21",
        )
        .execute(&pool)
        .await,
    );
    assert!(
        approval.contains("nur noch über den Autopiloten"),
        "{approval}"
    );
    sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) VALUES (31, 'tiktok', 'pending')")
        .execute(&pool)
        .await
        .expect("Kanal im Schattenbetrieb schreibt weiter");
    sqlx::query("DELETE FROM twitch_clips_upload_queue WHERE clip_id = 31")
        .execute(&pool)
        .await
        .expect("Aufräumen");

    let video: i64 = sqlx::query_scalar(
        "INSERT INTO social_produced_videos (twitch_user_id, clip_id, mode, status, file_path, duration_ms)
         VALUES ('300', 20, 'static', 'approved', '/tmp/u17.mp4', 15000) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("Version");
    let busy = rejection(
        sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id, platform, status, produced_video_id) VALUES (20, 'youtube', 'pending', $1)")
            .bind(video)
            .execute(&pool)
            .await,
    );
    assert!(busy.contains("schon eine Veröffentlichung"), "{busy}");

    assert_eq!(legacy_ids(&pool).await, vec![20]);
    sqlx::query("UPDATE twitch_clips_upload_queue SET status = 'processing' WHERE clip_id = 20")
        .execute(&pool)
        .await
        .expect("Zugesagter Beitrag läuft ab");
    sqlx::query("UPDATE twitch_clips_upload_queue SET status = 'pending' WHERE clip_id = 20")
        .execute(&pool)
        .await
        .expect("Zugesagter Beitrag bleibt erhalten");

    let map_before = map_fingerprint(&pool).await;
    let paused = pipeline_migration::pause(&pool, "300")
        .await
        .expect("Pause");
    assert_eq!(paused.outcome, CutoverOutcome::Paused);
    assert!(legacy_ids(&pool).await.is_empty());
    assert!(autopilot_paused(&pool, "300").await);
    sqlx::query(
        "INSERT INTO social_media_autopilot_settings (twitch_user_id, paused)
         SELECT twitch_user_id, paused FROM social_media_autopilot_settings WHERE twitch_user_id = '300'
         ON CONFLICT (twitch_user_id) DO UPDATE SET paused = EXCLUDED.paused, updated_at = NOW()",
    )
    .execute(&pool)
    .await
    .expect("Streamer speichert andere Einstellungen");
    let kept: Option<bool> = sqlx::query_scalar(
        "SELECT streamer_paused FROM social_pipeline_cutover WHERE twitch_user_id = '300'",
    )
    .fetch_one(&pool)
    .await
    .expect("Gesicherte Streamer-Pause");
    assert_eq!(kept, Some(false));
    sqlx::query(
        "INSERT INTO social_media_autopilot_settings (twitch_user_id, paused) VALUES ('300', FALSE)
         ON CONFLICT (twitch_user_id) DO UPDATE SET paused = EXCLUDED.paused, updated_at = NOW()",
    )
    .execute(&pool)
    .await
    .expect("Streamer hebt eigene Pause auf");
    assert!(autopilot_paused(&pool, "300").await);
    assert_eq!(
        pipeline_migration::pause(&pool, "300")
            .await
            .expect("Zweite Pause")
            .outcome,
        CutoverOutcome::NotUnified
    );
    let again = pipeline_migration::apply(&pool)
        .await
        .expect("Erneuter Lauf");
    assert!(again.skipped_unified >= 1);
    assert_eq!(
        pipeline_migration::activate(&pool, "300")
            .await
            .expect("Aktivierung")
            .outcome,
        CutoverOutcome::AlreadyClosed
    );
    let resumed = pipeline_migration::resume(&pool, "300")
        .await
        .expect("Fortsetzen");
    assert_eq!(resumed.outcome, CutoverOutcome::Resumed);
    assert_eq!(cutover_state(&pool, "300").await, "unified");
    assert_eq!(legacy_ids(&pool).await, vec![20]);
    assert_eq!(map_fingerprint(&pool).await, map_before);
    assert_eq!(legacy_counts(&pool).await, before);
    assert!(!autopilot_paused(&pool, "300").await);

    sqlx::query(
        "UPDATE social_media_autopilot_settings SET paused = TRUE WHERE twitch_user_id = '300'",
    )
    .execute(&pool)
    .await
    .expect("Streamer pausiert selbst");
    pipeline_migration::pause(&pool, "300")
        .await
        .expect("Pause über Streamer-Pause");
    pipeline_migration::resume(&pool, "300")
        .await
        .expect("Fortsetzen über Streamer-Pause");
    assert!(autopilot_paused(&pool, "300").await);
    pipeline_migration::pause(&pool, "300")
        .await
        .expect("Erneute Pause");
    sqlx::query(
        "INSERT INTO social_media_autopilot_settings (twitch_user_id, paused) VALUES ('300', FALSE)
         ON CONFLICT (twitch_user_id) DO UPDATE SET paused = EXCLUDED.paused, updated_at = NOW()",
    )
    .execute(&pool)
    .await
    .expect("Streamer hebt Pause über Speichern auf");
    assert!(autopilot_paused(&pool, "300").await);
    pipeline_migration::resume(&pool, "300")
        .await
        .expect("Fortsetzen nach Streamer-Wunsch");
    assert!(!autopilot_paused(&pool, "300").await);

    sqlx::query("UPDATE twitch_clips_upload_queue SET status = 'uncertain' WHERE clip_id = 20")
        .execute(&pool)
        .await
        .expect("Unbestätigter Beitrag");
    let command = CutoverCommand::parse(&["--social-cutover", "--resolve", "0", "--failed"]);
    assert_eq!(
        command,
        Some(CutoverCommand::Resolve {
            queue_id: 0,
            published: false
        })
    );
    let queue_id: i64 =
        sqlx::query_scalar("SELECT id FROM twitch_clips_upload_queue WHERE clip_id = 20")
            .fetch_one(&pool)
            .await
            .expect("Zeile");
    let resolved = pipeline_migration::run_command(
        &pool,
        &CutoverCommand::Resolve {
            queue_id,
            published: false,
        },
    )
    .await
    .expect("Auflösung");
    assert_eq!(resolved["resolved"], true);
    let status: String =
        sqlx::query_scalar("SELECT status FROM twitch_clips_upload_queue WHERE id = $1")
            .bind(queue_id)
            .fetch_one(&pool)
            .await
            .expect("Status");
    assert_eq!(status, "failed");
}
