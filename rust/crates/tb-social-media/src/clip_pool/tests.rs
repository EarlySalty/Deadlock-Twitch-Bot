use std::{str::FromStr, sync::Arc};

use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use wiremock::{
    matchers::{method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

use super::*;
use crate::{
    autopilot_settings::{self, AutopilotSettings, ClipDateFilter, ClipSort},
    clip::{helix::HelixClipSource, model::ClipRecord},
};

async fn pool(name: &str) -> PgPool {
    let dsn = crate::test_support::test_dsn().expect("owned local test database required");
    let schema = format!("t_m1_wf5f4_{name}");
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .unwrap();
    sqlx::query(crate::test_sql::drop_schema(&schema, true))
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query(crate::test_sql::create_schema(&schema, false))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    let options = PgConnectOptions::from_str(&dsn)
        .unwrap()
        .options([("search_path", schema.as_str())]);
    let pool = PgPoolOptions::new()
        .max_connections(6)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE TABLE twitch_streamers (twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT UNIQUE NOT NULL);
        INSERT INTO twitch_streamers VALUES ('channel', '42'), ('other', '99');
        CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT UNIQUE NOT NULL, twitch_user_id TEXT,
            clip_url TEXT, clip_title TEXT, clip_thumbnail_url TEXT, streamer_login TEXT, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            duration_seconds DOUBLE PRECISION, view_count INTEGER, game_id TEXT, game_name TEXT, category_key TEXT DEFAULT 'deadlock',
            status TEXT DEFAULT 'pending', source_kind TEXT NOT NULL DEFAULT 'twitch', vod_id TEXT, vod_offset_s INTEGER,
            local_file_path TEXT, upload_local_path TEXT, converted_file_path TEXT, layout_override_json JSONB,
            kontingent_verbraucht_at TIMESTAMPTZ, discarded_at TIMESTAMPTZ, download_failed_at TIMESTAMPTZ, downloaded_at TIMESTAMPTZ, retention_until TIMESTAMPTZ,
            uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE,
            youtube_video_id TEXT, tiktok_video_id TEXT, instagram_media_id TEXT, youtube_uploaded_at TIMESTAMPTZ,
            tiktok_uploaded_at TIMESTAMPTZ, instagram_uploaded_at TIMESTAMPTZ, custom_description TEXT, hashtags TEXT);
        CREATE TABLE social_media_category (category_key TEXT PRIMARY KEY, enrichment_enabled BOOLEAN NOT NULL DEFAULT TRUE);
        INSERT INTO social_media_category VALUES ('deadlock', TRUE);
        CREATE TABLE social_media_streamer_layout (twitch_user_id TEXT PRIMARY KEY, layout_json JSONB, cam_enabled BOOLEAN, mode TEXT);
        CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT NOT NULL DEFAULT 'awaiting_approval',
            approved_platforms JSONB NOT NULL DEFAULT '[]', approver_user_id TEXT, decided_at TIMESTAMPTZ, dm_message_id TEXT,
            dm_channel_id TEXT, last_sent_at TIMESTAMPTZ, letzter_nachreih_versuch TIMESTAMPTZ);
        CREATE TABLE social_media_clip_enrichment (clip_db_id INTEGER PRIMARY KEY, status TEXT DEFAULT 'pending');
        CREATE TABLE twitch_clips_upload_queue (id BIGSERIAL PRIMARY KEY, clip_id BIGINT NOT NULL, platform TEXT NOT NULL,
            status TEXT DEFAULT 'pending', scheduled_at TIMESTAMPTZ, title TEXT, description TEXT, hashtags TEXT, priority INTEGER DEFAULT 0,
            attempts INTEGER DEFAULT 0, quota_deferrals INTEGER DEFAULT 0, last_error TEXT, last_attempt_at TIMESTAMPTZ,
            created_at TIMESTAMPTZ DEFAULT NOW(), completed_at TIMESTAMPTZ)")
        .execute(&pool).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261011100000_social_autopilot_contracts.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261011101000_social_clip_pool.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    pool
}

fn timestamp(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

fn clip(id: &str, created: &str) -> ClipRecord {
    ClipRecord {
        clip_id: id.into(),
        clip_url: format!("https://clips.twitch.tv/{id}"),
        clip_title: id.into(),
        thumbnail_url: None,
        streamer_login: "channel".into(),
        twitch_user_id: "42".into(),
        broadcaster_name: None,
        created_at: created.into(),
        duration_seconds: 30.0,
        view_count: 100,
        game_name: None,
        game_id: Some(DEADLOCK_GAME_ID.into()),
        vod_id: None,
        vod_offset_s: None,
    }
}

fn raw(id: &str, owner: &str, game: &str, created: &str) -> serde_json::Value {
    json!({"id":id,"url":format!("https://clips.twitch.tv/{id}"),"title":id,"broadcaster_id":owner,
        "game_id":game,"created_at":created,"duration":30.0,"view_count":100,"creator_id":"unrelated_viewer"})
}

async fn source() -> (MockServer, HelixClipSource) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"access_token":"local-fixture","expires_in":3600})),
        )
        .mount(&server)
        .await;
    let client = tb_transport_twitch::HelixClient::new(tb_transport_twitch::HelixConfig {
        client_id: "local-fixture".into(),
        client_secret: "local-fixture".into(),
        token_url: format!("{}/oauth2/token", server.uri()),
        helix_base: format!("{}/helix", server.uri()),
    })
    .unwrap();
    (server, HelixClipSource::new(Arc::new(client)))
}

async fn year(pool: &PgPool, value: i32) {
    save_preferences(
        pool,
        "42",
        &PoolPreferences {
            clip_date_filter: ClipDateFilter::Year { year: value },
            clip_sort: ClipSort::OldestFirst,
            timezone: "Europe/Berlin".into(),
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn additive_pool_schema_matches_existing_snapshot_contract() {
    let pool = pool("snapshot").await;
    let actual: Vec<String> = sqlx::query_scalar("SELECT table_name || '|' || column_name || '|' || data_type || '|' || is_nullable || '|' || COALESCE(column_default, '')
        FROM information_schema.columns WHERE table_schema = current_schema()
        AND (table_name IN ('social_clip_import_jobs', 'social_clip_pool', 'social_clip_pool_owners')
            OR (table_name = 'twitch_clips_social_media' AND column_name IN ('pool_managed_at', 'pool_legacy_reserved', 'pool_legacy_prerequisite')))")
        .fetch_all(&pool).await.unwrap();
    let expected = include_str!("../../../../crates/tb-db/tests/fresh_schema_snapshot.txt")
        .lines()
        .filter(|line| {
            line.starts_with("social_clip_") || line.starts_with("twitch_clips_social_media|pool_")
        })
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        actual
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        expected
    );
    assert_eq!(expected.len(), 41);
}

#[test]
fn calendar_bounds_and_strict_identity_are_independent_of_viewer_and_name() {
    let now = timestamp("2026-10-11T12:00:00Z");
    let mut settings = AutopilotSettings {
        clip_date_filter: ClipDateFilter::Quarter {
            year: 2025,
            quarter: 1,
        },
        ..Default::default()
    };
    let window = bounds(&settings, now).unwrap();
    assert_eq!(window.start, timestamp("2024-12-31T23:00:00Z"));
    assert_eq!(window.end, timestamp("2025-03-31T22:00:00Z"));
    let mut value = clip("a", "2024-12-31T23:00:00Z");
    assert!(eligible("42", &value, &window).unwrap());
    value.twitch_user_id = "99".into();
    assert!(!eligible("42", &value, &window).unwrap());
    value.twitch_user_id = "42".into();
    value.game_id = Some("other".into());
    value.game_name = Some("Deadlock".into());
    assert!(!eligible("42", &value, &window).unwrap());
    value.game_id = None;
    assert!(eligible("42", &value, &window).is_err());
    value.game_id = Some(DEADLOCK_GAME_ID.into());
    value.created_at = "invalid".into();
    assert!(eligible("42", &value, &window).is_err());
    settings.clip_date_filter = ClipDateFilter::Before {
        date: "2025-03-30".parse().unwrap(),
    };
    assert_eq!(
        bounds(&settings, now).unwrap().end,
        timestamp("2025-03-29T23:00:00Z")
    );
    settings.clip_date_filter = ClipDateFilter::After {
        date: "2025-03-30".parse().unwrap(),
    };
    assert_eq!(
        bounds(&settings, now).unwrap().start,
        timestamp("2025-03-30T22:00:00Z")
    );
    settings.timezone = "Pacific/Apia".into();
    settings.clip_date_filter = ClipDateFilter::Range {
        start: "2011-12-30".parse().unwrap(),
        end: "2011-12-31".parse().unwrap(),
    };
    assert_eq!(
        bounds(&settings, now).unwrap().start,
        timestamp("2011-12-30T10:00:00Z")
    );
    settings.clip_date_filter = ClipDateFilter::Quarter {
        year: 2025,
        quarter: 0,
    };
    assert!(bounds(&settings, now).is_err());
}

#[tokio::test]
async fn historical_pages_are_atomic_metadata_only_and_ordered_by_source_date() {
    let pool = pool("pages").await;
    year(&pool, 2025).await;
    assert!(!is_managed(&pool, "42").await.unwrap());
    let (server, source) = source().await;
    let first = Mock::given(path("/helix/clips")).and(query_param("broadcaster_id", "42"))
        .and(query_param("first", "100")).and(query_param("started_at", "2024-12-31T23:00:00Z"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[
            raw("wrong-owner", "99", DEADLOCK_GAME_ID, "2025-01-01T00:00:00Z"),
            raw("wrong-game", "42", "other", "2025-01-01T00:00:00Z"),
            raw("too-new", "42", DEADLOCK_GAME_ID, "2026-01-01T00:00:00Z")], "pagination":{"cursor":"second"}})))
        .up_to_n_times(1).expect(1).mount_as_scoped(&server).await;
    Mock::given(path("/helix/clips")).and(query_param("after", "second"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[
            raw("b", "42", DEADLOCK_GAME_ID, "2025-12-01T00:00:00Z"),
            raw("a", "42", DEADLOCK_GAME_ID, "2025-01-01T00:00:00Z"),
            raw("c", "42", DEADLOCK_GAME_ID, "2025-12-01T00:00:00Z")],"pagination":{"cursor":"third"}})))
        .expect(1).mount(&server).await;
    Mock::given(path("/helix/clips")).and(query_param("after", "third"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[raw("a", "42", DEADLOCK_GAME_ID, "2025-01-01T00:00:00Z")],"pagination":{}})))
        .expect(1).mount(&server).await;
    let job = start_import(&pool, "42", "channel").await.unwrap();
    let same = start_import(&pool, "42", "channel").await.unwrap();
    assert_eq!(job.id, same.id);
    year(&pool, 2024).await;
    let worker = ClipImportWorker::new(pool.clone(), source);
    assert!(worker.run_once().await.unwrap());
    drop(first);
    assert_eq!(
        get_import(&pool, "42", job.id).await.unwrap().raw_fetched,
        3
    );
    assert!(worker.run_once().await.unwrap());
    assert!(worker.run_once().await.unwrap());
    let done = get_import(&pool, "42", job.id).await.unwrap();
    assert_eq!(
        (
            done.status.as_str(),
            done.raw_fetched,
            done.eligible_fetched,
            done.persisted_count
        ),
        ("completed", 7, 4, 3)
    );
    assert_eq!(done.timezone, "Europe/Berlin");
    year(&pool, 2025).await;
    let page = list(&pool, "42", 50, 0).await.unwrap();
    assert_eq!(
        page.clips
            .iter()
            .map(|c| c.clip_id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
    assert!(page.clips.iter().all(|c| c.selectable));
    save_preferences(
        &pool,
        "42",
        &PoolPreferences {
            clip_sort: ClipSort::NewestFirst,
            ..page.preferences
        },
    )
    .await
    .unwrap();
    assert_eq!(
        list(&pool, "42", 50, 0)
            .await
            .unwrap()
            .clips
            .iter()
            .map(|c| c.clip_id.as_str())
            .collect::<Vec<_>>(),
        ["b", "c", "a"]
    );
    set_selected(&pool, "42", "a", true).await.unwrap();
    assert!(matches!(
        set_selected(&pool, "99", "a", true).await,
        Err(PoolError::NotFound)
    ));
    year(&pool, 2024).await;
    let page = list(&pool, "42", 50, 0).await.unwrap();
    assert!(page.clips.iter().all(|c| !c.eligible));
    assert!(page
        .clips
        .iter()
        .find(|c| c.clip_id == "a")
        .unwrap()
        .selected_at
        .is_some());
    assert!(matches!(
        set_selected(&pool, "42", "b", true).await,
        Err(PoolError::NotSelectable)
    ));
    set_selected(&pool, "42", "a", false).await.unwrap();
    assert!(list(&pool, "42", 50, 0)
        .await
        .unwrap()
        .clips
        .iter()
        .find(|c| c.clip_id == "a")
        .unwrap()
        .first_selected_at
        .is_some());
    assert_eq!(
        register_recent(&pool, "42", &[clip("recent", "2026-10-11T00:00:00Z")])
            .await
            .unwrap(),
        (0, 0)
    );
    let aggregates: (i64,i64,i64,i64,i64,i64) = sqlx::query_as("SELECT
        (SELECT COUNT(*) FROM social_production_jobs), (SELECT COUNT(*) FROM twitch_clips_upload_queue),
        (SELECT COUNT(*) FROM social_media_clip_approval), (SELECT COUNT(*) FROM social_media_clip_enrichment),
        (SELECT COUNT(*) FROM twitch_clips_social_media WHERE local_file_path IS NOT NULL OR converted_file_path IS NOT NULL),
        (SELECT COUNT(*) FROM twitch_clips_social_media WHERE kontingent_verbraucht_at IS NOT NULL)")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(aggregates, (0, 0, 0, 0, 0, 0));
    server.verify().await;
}

#[tokio::test]
async fn concurrent_registration_preserves_layout_files_and_durable_history() {
    let pool = pool("dedup").await;
    year(&pool, 2025).await;
    repository::manage_on(&mut pool.acquire().await.unwrap(), "42")
        .await
        .unwrap();
    let value = clip("shared", "2025-04-01T00:00:00Z");
    let (one, two) = tokio::join!(
        register_recent(&pool, "42", std::slice::from_ref(&value)),
        register_recent(&pool, "42", std::slice::from_ref(&value))
    );
    assert_eq!(one.unwrap().1 + two.unwrap().1, 1);
    let id = list(&pool, "42", 50, 0).await.unwrap().clips[0]
        .clip_db_id
        .unwrap();
    set_selected(&pool, "42", "shared", true).await.unwrap();
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("render.mp4");
    tokio::fs::write(&file, b"retained real file")
        .await
        .unwrap();
    sqlx::query("UPDATE twitch_clips_social_media SET converted_file_path = $2, layout_override_json = '{\"owned_override\":true}', uploaded_youtube = TRUE WHERE id = $1")
        .bind(id).bind(file.to_str().unwrap()).execute(&pool).await.unwrap();
    register_recent(&pool, "42", std::slice::from_ref(&value))
        .await
        .unwrap();
    let retained: (String, serde_json::Value, bool) = sqlx::query_as("SELECT converted_file_path, layout_override_json, uploaded_youtube FROM twitch_clips_social_media WHERE id = $1")
        .bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(
        retained,
        (
            file.to_str().unwrap().into(),
            json!({"owned_override":true}),
            true
        )
    );
    assert!(matches!(
        set_selected(&pool, "42", "shared", false).await,
        Err(PoolError::Reserved)
    ));
    tokio::fs::remove_file(&file).await.unwrap();
    sqlx::query("DELETE FROM twitch_clips_social_media WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let row = &list(&pool, "42", 50, 0).await.unwrap().clips[0];
    assert!(
        row.clip_db_id.is_none()
            && row.processed_at.is_some()
            && row.published_at.is_some()
            && row.first_selected_at.is_some()
    );
    register_recent(&pool, "42", std::slice::from_ref(&value))
        .await
        .unwrap();
    assert_eq!(list(&pool, "42", 50, 0).await.unwrap().total, 1);
    assert!(matches!(
        set_selected(&pool, "42", "shared", true).await,
        Err(PoolError::NotSelectable)
    ));
    let foreign = ClipRecord {
        twitch_user_id: "99".into(),
        ..value
    };
    let window = ImportWindow::new(
        timestamp("2025-01-01T00:00:00Z"),
        timestamp("2026-01-01T00:00:00Z"),
    );
    let mut tx = pool.begin().await.unwrap();
    assert!(repository::register_on(&mut tx, &foreign, &window)
        .await
        .is_err());
}

#[tokio::test]
async fn leases_retry_reset_invalid_cursors_and_failed_pages_do_not_advance() {
    let pool = pool("recovery").await;
    year(&pool, 2025).await;
    let (server, source) = source().await;
    let job = start_import(&pool, "42", "channel").await.unwrap();
    let worker = ClipImportWorker::new(pool.clone(), source.clone());
    let first = Mock::given(path("/helix/clips"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[raw("a", "42", DEADLOCK_GAME_ID, "2025-05-01T00:00:00Z")], "pagination":{"cursor":"expired"}})))
        .expect(1).mount_as_scoped(&server).await;
    worker.run_once().await.unwrap();
    drop(first);
    let reset = timestamp("2099-01-01T00:00:00Z");
    let limited = Mock::given(path("/helix/clips"))
        .and(query_param("after", "expired"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Ratelimit-Reset", reset.timestamp().to_string()),
        )
        .expect(1)
        .mount_as_scoped(&server)
        .await;
    worker.run_once().await.unwrap();
    drop(limited);
    let retry = get_import(&pool, "42", job.id).await.unwrap();
    assert_eq!(retry.next_retry_at, Some(reset));
    assert_eq!(retry.raw_fetched, 1);
    assert!(!worker.run_once().await.unwrap());
    sqlx::query("UPDATE social_clip_import_jobs SET next_retry_at = '2000-01-01' WHERE id = $1")
        .bind(job.id)
        .execute(&pool)
        .await
        .unwrap();
    let invalid = Mock::given(path("/helix/clips"))
        .and(query_param("after", "expired"))
        .respond_with(ResponseTemplate::new(400))
        .expect(1)
        .mount_as_scoped(&server)
        .await;
    worker.run_once().await.unwrap();
    drop(invalid);
    let cursor: Option<String> =
        sqlx::query_scalar("SELECT cursor FROM social_clip_import_jobs WHERE id = $1")
            .bind(job.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(cursor.is_none());
    sqlx::raw_sql("CREATE FUNCTION fail_pool_page() RETURNS TRIGGER LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test write failure'; END; $$;
        CREATE TRIGGER fail_pool_page BEFORE INSERT ON social_clip_pool FOR EACH ROW EXECUTE FUNCTION fail_pool_page()")
        .execute(&pool).await.unwrap();
    let repeated = Mock::given(path("/helix/clips"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"data":[raw("a", "42", DEADLOCK_GAME_ID, "2025-05-01T00:00:00Z"),
            raw("b", "42", DEADLOCK_GAME_ID, "2025-06-01T00:00:00Z")], "pagination":{}}),
        ))
        .expect(2)
        .mount_as_scoped(&server)
        .await;
    let resumed_worker = ClipImportWorker::new(pool.clone(), source);
    assert!(resumed_worker.run_once().await.is_err());
    let failed = get_import(&pool, "42", job.id).await.unwrap();
    assert_eq!((failed.raw_fetched, failed.persisted_count), (1, 1));
    assert_eq!(list(&pool, "42", 50, 0).await.unwrap().total, 1);
    sqlx::raw_sql(
        "DROP TRIGGER fail_pool_page ON social_clip_pool; DROP FUNCTION fail_pool_page();
        UPDATE social_clip_import_jobs SET next_retry_at = '2000-01-01'",
    )
    .execute(&pool)
    .await
    .unwrap();
    resumed_worker.run_once().await.unwrap();
    drop(repeated);
    let done = get_import(&pool, "42", job.id).await.unwrap();
    assert_eq!(
        (done.status.as_str(), done.raw_fetched, done.persisted_count),
        ("completed", 3, 2)
    );
    assert_eq!(list(&pool, "42", 50, 0).await.unwrap().total, 2);
    let another = start_import(&pool, "42", "channel").await.unwrap();
    sqlx::query("UPDATE social_clip_import_jobs SET status = 'running', lease_until = '2099-01-01' WHERE id = $1")
        .bind(another.id).execute(&pool).await.unwrap();
    assert!(!resumed_worker.run_once().await.unwrap());
    assert!(matches!(
        resume_import(&pool, "42", another.id).await,
        Err(PoolError::Busy)
    ));
    assert!(matches!(
        resume_import(&pool, "99", another.id).await,
        Err(PoolError::NotFound)
    ));
    server.verify().await;
}

#[tokio::test]
async fn saturated_windows_split_and_unsplittable_windows_remain_incomplete() {
    for (name, width, expected) in [("split", 10, "queued"), ("dense", 1, "incomplete")] {
        let pool = pool(name).await;
        year(&pool, 2025).await;
        let job = start_import(&pool, "42", "channel").await.unwrap();
        let start = timestamp("2025-01-01T00:00:00Z");
        let window = ImportWindow::new(start, start + chrono::Duration::seconds(width));
        sqlx::query("UPDATE social_clip_import_jobs SET pending_windows = $2 WHERE id = $1")
            .bind(job.id)
            .bind(json!([window]))
            .execute(&pool)
            .await
            .unwrap();
        let (server, source) = source().await;
        let worker = ClipImportWorker::new(pool.clone(), source);
        for page in 0..9 {
            let data = (0..100)
                .map(|id| {
                    raw(
                        &format!("dense-{id}"),
                        "42",
                        DEADLOCK_GAME_ID,
                        "2025-01-01T00:00:00Z",
                    )
                })
                .collect::<Vec<_>>();
            let next = (page < 8).then(|| format!("page-{}", page + 1));
            let mut request = Mock::given(path("/helix/clips"));
            if page > 0 {
                request = request.and(query_param("after", format!("page-{page}")));
            }
            let page_mock = request
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({"data":data,"pagination":{"cursor":next}})),
                )
                .expect(1)
                .mount_as_scoped(&server)
                .await;
            worker.run_once().await.unwrap();
            drop(page_mock);
        }
        let result = get_import(&pool, "42", job.id).await.unwrap();
        assert_eq!(
            (
                result.status.as_str(),
                result.raw_fetched,
                result.persisted_count
            ),
            (expected, 900, 100)
        );
        assert_eq!(list(&pool, "42", 200, 0).await.unwrap().total, 100);
        if width > 1 {
            assert_eq!(result.pending_windows, 2);
            assert!(result.incomplete_windows.is_empty());
        } else {
            assert_eq!(result.incomplete_windows.len(), 1);
            let resumed = resume_import(&pool, "42", job.id).await.unwrap();
            assert_eq!(
                (resumed.status.as_str(), resumed.pending_windows),
                ("queued", 1)
            );
            assert_eq!(resumed.started_at, job.started_at);
            let empty = Mock::given(path("/helix/clips"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"data":[], "pagination":{}})),
                )
                .expect(1)
                .mount_as_scoped(&server)
                .await;
            worker.run_once().await.unwrap();
            drop(empty);
            let completed = get_import(&pool, "42", job.id).await.unwrap();
            assert_eq!(completed.status, "completed");
            assert_eq!(completed.raw_fetched, 900);
            assert!(completed.incomplete_windows.is_empty());
        }
    }
}

#[tokio::test]
async fn http_and_missing_authority_data_never_finish_as_empty_success() {
    let pool = pool("invalid").await;
    year(&pool, 2025).await;
    let job = start_import(&pool, "42", "channel").await.unwrap();
    let (server, source) = source().await;
    let worker = ClipImportWorker::new(pool.clone(), source);
    for response in [
        ResponseTemplate::new(403),
        ResponseTemplate::new(200).set_body_json(
            json!({"data":[{"id":"incomplete","url":"https://clips.twitch.tv/x"}],"pagination":{}}),
        ),
    ] {
        let mock = Mock::given(path("/helix/clips"))
            .respond_with(response)
            .expect(1)
            .mount_as_scoped(&server)
            .await;
        worker.run_once().await.unwrap();
        drop(mock);
        let failed = get_import(&pool, "42", job.id).await.unwrap();
        assert_eq!(
            (
                failed.status.as_str(),
                failed.raw_fetched,
                failed.persisted_count
            ),
            ("retry", 0, 0)
        );
        sqlx::query(
            "UPDATE social_clip_import_jobs SET next_retry_at = '2000-01-01' WHERE id = $1",
        )
        .bind(job.id)
        .execute(&pool)
        .await
        .unwrap();
    }
    assert_eq!(list(&pool, "42", 50, 0).await.unwrap().total, 0);
}

struct FileDownloader;

#[async_trait::async_trait]
impl crate::clip_prep_worker::ClipDownloader for FileDownloader {
    async fn download(&self, _url: &str, destination: &std::path::Path) -> Result<(), String> {
        tokio::fs::write(destination, b"legacy prerequisite")
            .await
            .map_err(|error| error.to_string())
    }
}

struct NeverLlm;

#[async_trait::async_trait]
impl crate::enrich_pipeline::EnrichmentLlm for NeverLlm {
    async fn generate(
        &self,
        _: &crate::llm::LlmRequest,
    ) -> Result<crate::llm::LlmResponse, crate::llm::LlmError> {
        panic!("pool import must not call a model")
    }
}

#[tokio::test]
async fn legacy_selectors_are_isolated_but_existing_upload_prerequisites_survive() {
    let pool = pool("guards").await;
    sqlx::query("INSERT INTO twitch_clips_social_media (clip_id, twitch_user_id, streamer_login, clip_url, game_id, created_at)
        VALUES ('reserved', '42', 'channel', 'https://clips.twitch.tv/reserved', $1, '2025-06-01'),
            ('idle', '42', 'channel', 'https://clips.twitch.tv/idle', $1, '2025-06-02')")
        .bind(DEADLOCK_GAME_ID).execute(&pool).await.unwrap();
    let reserved: i64 =
        sqlx::query_scalar("SELECT id FROM twitch_clips_social_media WHERE clip_id = 'reserved'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let idle: i64 =
        sqlx::query_scalar("SELECT id FROM twitch_clips_social_media WHERE clip_id = 'idle'")
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id, platform, created_at) VALUES ($1, 'youtube', '2025-06-01')")
        .bind(reserved).execute(&pool).await.unwrap();
    autopilot_settings::save(
        &pool,
        "42",
        &AutopilotSettings {
            enabled: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(is_managed(&pool, "42").await.unwrap());
    autopilot_settings::save(&pool, "99", &AutopilotSettings::default())
        .await
        .unwrap();
    assert!(!is_managed(&pool, "99").await.unwrap());
    assert!(crate::approval::iter_clips_ohne_enrichment(&pool, 100)
        .await
        .is_empty());
    let directory = tempfile::tempdir().unwrap();
    let prep = crate::clip_prep_worker::ClipPrepWorker::new(pool.clone(), "unused")
        .with_clips_dir(directory.path().to_str().unwrap())
        .with_downloader(Arc::new(FileDownloader));
    assert_eq!(prep.prepare_once().await, 1);
    assert!(directory.path().join(format!("{reserved}.mp4")).is_file());
    assert!(!directory.path().join(format!("{idle}.mp4")).exists());
    assert!(crate::enrichment::iter_pending_enrichments(&pool, 100)
        .await
        .is_empty());
    crate::approval::mark_clip_awaiting_approval(&pool, idle as i32).await;
    assert!(crate::approval::auto_approve_if_allowed(&pool, idle as i32)
        .await
        .is_empty());
    assert!(matches!(
        crate::enrich_pipeline::ClipEnrichmentPipeline::new(pool.clone())
            .run(idle as i32, None, &NeverLlm, true)
            .await,
        Err(crate::enrich_pipeline::PipelineError::PoolManaged)
    ));
    assert_eq!(
        crate::clip_manager::batch_upload_all_new(&pool, "42", &["youtube".into()], false)
            .await
            .queued,
        0
    );
    assert!(
        sqlx::query("INSERT INTO social_media_clip_approval (clip_db_id) VALUES ($1)")
            .bind(idle as i32)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(sqlx::query(
        "INSERT INTO twitch_clips_upload_queue (clip_id, platform) VALUES ($1, 'instagram')"
    )
    .bind(reserved)
    .execute(&pool)
    .await
    .is_err());
    let mut tx = pool.begin().await.unwrap();
    assert!(matches!(
        crate::clip_queue::queue_upload_on(
            &mut tx,
            crate::clip_queue::QueueUploadRequest {
                clip_db_id: idle,
                platform: "youtube",
                title: None,
                description: None,
                hashtags: None,
                schedule: crate::clip_queue::UploadSchedule::Preserve(None),
                priority: 0,
            },
            None
        )
        .await,
        Err(crate::clip_queue::QueueError::PoolManaged)
    ));
    tx.rollback().await.unwrap();
    sqlx::query("UPDATE twitch_clips_upload_queue SET status = 'completed', completed_at = NOW() WHERE clip_id = $1")
        .bind(reserved).execute(&pool).await.unwrap();
    sqlx::query("UPDATE twitch_clips_social_media SET local_file_path = NULL WHERE id = $1")
        .bind(reserved)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(prep.prepare_once().await, 0);
    let history = list(&pool, "42", 50, 0).await.unwrap();
    assert!(history
        .clips
        .iter()
        .find(|clip| clip.clip_id == "reserved")
        .unwrap()
        .published_at
        .is_some());
    let counts: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT COUNT(*) FROM twitch_clips_upload_queue),
        (SELECT COUNT(*) FROM social_production_jobs), (SELECT COUNT(*) FROM social_media_clip_approval)")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(counts, (1, 0, 0));
}

#[tokio::test]
async fn filter_changes_and_deselection_preserve_running_work_and_exact_file_events() {
    use crate::autopilot_settings::ScheduleSlot;
    use crate::produced_video::{self, NewProducedVideo, RenderedVideo};
    use crate::production_jobs::ProductionMode;

    let pool = pool("preservation").await;
    year(&pool, 2025).await;
    repository::manage_on(&mut pool.acquire().await.unwrap(), "42")
        .await
        .unwrap();
    register_recent(&pool, "42", &[clip("protected", "2025-07-01T00:00:00Z")])
        .await
        .unwrap();
    let clip_id = list(&pool, "42", 50, 0).await.unwrap().clips[0]
        .clip_db_id
        .unwrap();
    set_selected(&pool, "42", "protected", true).await.unwrap();
    sqlx::query("INSERT INTO social_production_jobs (twitch_user_id, clip_id, source, requested_mode, mode, queue_order, status, started_at)
        VALUES ('42', $1, 'manual', 'ai', 'ai', 1, 'running', NOW())")
        .bind(clip_id).execute(&pool).await.unwrap();
    let video = produced_video::insert(
        &pool,
        &NewProducedVideo {
            twitch_user_id: "42".into(),
            clip_id,
            job_id: None,
            mode: ProductionMode::Ai,
            version_no: 1,
            parent_version_id: None,
            variant_index: 0,
        },
    )
    .await
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("exact.mp4");
    tokio::fs::write(&file, b"exact retained fixture")
        .await
        .unwrap();
    produced_video::complete_render(
        &pool,
        "42",
        video.id,
        &RenderedVideo {
            file_path: file.to_str().unwrap().into(),
            duration_ms: 15000,
            needs_review: false,
            review_reasons: Vec::new(),
            metadata: json!({"platforms":{"youtube":{"title":"Fixture"}}}),
        },
    )
    .await
    .unwrap();
    let mut settings = autopilot_settings::load(&pool, "42").await.unwrap();
    settings.schedule_slots = (1..=7)
        .map(|weekday| ScheduleSlot {
            weekday,
            local_time: "18:00".into(),
        })
        .collect();
    autopilot_settings::save(&pool, "42", &settings)
        .await
        .unwrap();
    produced_video::approve(&pool, "42", video.id, &["youtube".into()])
        .await
        .unwrap();
    let event = produced_video::enqueue_for_publishing(&pool, video.id, &["youtube".into()])
        .await
        .unwrap();
    let before: serde_json::Value = sqlx::query_scalar("SELECT jsonb_build_object('jobs', (SELECT jsonb_agg(to_jsonb(j)) FROM social_production_jobs j),
        'videos', (SELECT jsonb_agg(to_jsonb(v)) FROM social_produced_videos v), 'queue', (SELECT jsonb_agg(to_jsonb(q)) FROM twitch_clips_upload_queue q))")
        .fetch_one(&pool).await.unwrap();
    year(&pool, 2024).await;
    assert!(matches!(
        set_selected(&pool, "42", "protected", false).await,
        Err(PoolError::Reserved)
    ));
    let after: serde_json::Value = sqlx::query_scalar("SELECT jsonb_build_object('jobs', (SELECT jsonb_agg(to_jsonb(j)) FROM social_production_jobs j),
        'videos', (SELECT jsonb_agg(to_jsonb(v)) FROM social_produced_videos v), 'queue', (SELECT jsonb_agg(to_jsonb(q)) FROM twitch_clips_upload_queue q))")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(before, after);
    assert_eq!(event.queue_ids.len(), 1);
    assert_eq!(
        tokio::fs::read(file).await.unwrap(),
        b"exact retained fixture"
    );
    let row = &list(&pool, "42", 50, 0).await.unwrap().clips[0];
    assert!(
        row.selected_at.is_some() && row.processed_at.is_some() && !row.selectable && !row.eligible
    );
}
