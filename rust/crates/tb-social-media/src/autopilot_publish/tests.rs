use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;

use super::*;
use crate::autopilot_settings::{AutopilotSettings, ScheduleSlot};
use crate::produced_video::{NewProducedVideo, RenderedVideo};
use crate::production_jobs::ProductionMode;
use crate::uploaders::tiktok::TikTokPostOptions;
use crate::uploaders::{UploadCheckpoint, UploadError};

static NEXT: AtomicU64 = AtomicU64::new(0);

async fn pool() -> PgPool {
    let dsn = crate::test_support::test_dsn().expect("isolated test database is required");
    let name = format!(
        "t_sm_c_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("test database");
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
        .execute(&admin)
        .await
        .expect("owned database");
    admin.close().await;
    let options = PgConnectOptions::from_str(&dsn)
        .expect("test DSN")
        .database(&name);
    let pool = PgPoolOptions::new()
        .max_connections(6)
        .connect_with(options)
        .await
        .expect("test pool");
    sqlx::raw_sql("CREATE TABLE twitch_streamers (twitch_user_id TEXT PRIMARY KEY);
        INSERT INTO twitch_streamers VALUES ('owner'), ('other');
        CREATE TABLE social_media_settings (key TEXT PRIMARY KEY, value JSONB NOT NULL, updated_at TIMESTAMPTZ DEFAULT NOW(), updated_by TEXT);
        CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT NOT NULL, twitch_login TEXT NOT NULL, discord_user_id TEXT, updated_at TEXT);
        CREATE TABLE social_media_platform_auth (id SERIAL PRIMARY KEY, platform TEXT NOT NULL, twitch_user_id TEXT,
            platform_user_id TEXT, enabled INTEGER DEFAULT 1, authorized_at TEXT DEFAULT CURRENT_TIMESTAMP);
        CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT, clip_id TEXT,
            clip_url TEXT, clip_title TEXT, streamer_login TEXT, local_file_path TEXT, converted_file_path TEXT,
            status TEXT DEFAULT 'pending', game_id TEXT DEFAULT '2132205352', source_kind TEXT DEFAULT 'twitch',
            clip_thumbnail_url TEXT, duration_seconds DOUBLE PRECISION DEFAULT 15, created_at TIMESTAMPTZ DEFAULT NOW(),
            uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE,
            youtube_video_id TEXT, tiktok_video_id TEXT, instagram_media_id TEXT,
            youtube_uploaded_at TIMESTAMPTZ, tiktok_uploaded_at TIMESTAMPTZ, instagram_uploaded_at TIMESTAMPTZ);
        CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT NOT NULL DEFAULT 'awaiting_approval', approved_platforms JSONB NOT NULL DEFAULT '[]', approver_user_id TEXT, decided_at TIMESTAMPTZ);
        CREATE TABLE twitch_clips_upload_queue (id BIGSERIAL PRIMARY KEY, clip_id BIGINT NOT NULL, platform TEXT NOT NULL,
            status TEXT DEFAULT 'pending', scheduled_at TIMESTAMPTZ, title TEXT, description TEXT, hashtags TEXT,
            priority INTEGER DEFAULT 0, attempts INTEGER DEFAULT 0, last_error TEXT, last_attempt_at TIMESTAMPTZ,
            completed_at TIMESTAMPTZ, youtube_visibility TEXT, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            tiktok_publish_id TEXT, tiktok_post_options JSONB)")
        .execute(&pool).await.expect("legacy prerequisite schema");
    for migration in [
        include_str!("../../../../migrations/20261007042000_social_media_tiktok_direct_post.sql"),
        include_str!("../../../../migrations/20261011100000_social_autopilot_contracts.sql"),
        include_str!("../../../../migrations/20261011101000_social_clip_pool.sql"),
        include_str!("../../../../migrations/20261011102000_social_production_slot_guards.sql"),
        include_str!("../../../../migrations/20261013140000_social_autopilot_publishing.sql"),
        include_str!("../../../../migrations/20261015124000_social_pipeline_policy.sql"),
        include_str!("../../../../migrations/20261015180000_social_destination_slots.sql"),
        include_str!("../../../../migrations/20261018120000_social_production_test_run.sql"),
        include_str!("../../../../migrations/20261018130000_social_test_run_discard_release.sql"),
        include_str!("../../../../migrations/20261018140000_social_test_run_revision_release.sql"),
    ] {
        sqlx::raw_sql(migration)
            .execute(&pool)
            .await
            .expect("actual migration");
    }
    pool
}

async fn cleanup(pool: PgPool) {
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .expect("database name");
    assert!(database.starts_with(&format!("t_sm_c_{}_", std::process::id())));
    pool.close().await;
    let dsn = crate::test_support::test_dsn().expect("isolated test database");
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("admin");
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP DATABASE {database}")))
        .execute(&admin)
        .await
        .expect("drop database");
    admin.close().await;
}

async fn configure(pool: &PgPool, targets: &[&str], connected: &[(&str, &str)]) {
    let settings = AutopilotSettings {
        schedule_slots: (1..=7)
            .flat_map(|weekday| {
                ["06:00", "12:00", "18:00"].map(|time| ScheduleSlot {
                    weekday,
                    local_time: time.into(),
                })
            })
            .collect(),
        target_platforms: targets.iter().map(|value| value.to_string()).collect(),
        ..AutopilotSettings::default()
    };
    autopilot_settings::save(pool, "owner", &settings)
        .await
        .expect("settings");
    for (platform, account) in connected {
        sqlx::query("INSERT INTO social_media_platform_auth (platform, twitch_user_id, platform_user_id) VALUES ($1, 'owner', $2)")
            .bind(platform)
            .bind(account)
            .execute(pool)
            .await
            .expect("connection");
    }
}

async fn video(pool: &PgPool, name: &str) -> (i64, std::path::PathBuf) {
    let clip: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (twitch_user_id, clip_id, clip_title, streamer_login, clip_url) VALUES ('owner', $1, $1, 'owner', 'https://clips.test') RETURNING id::bigint")
        .bind(name)
        .fetch_one(pool)
        .await
        .expect("clip");
    let directory = std::env::temp_dir().join(format!(
        "tb-publish-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).expect("directory");
    let file = directory.join("video.mp4");
    std::fs::write(&file, b"exact rendered bytes").expect("file");
    let video = produced_video::insert(
        pool,
        &NewProducedVideo {
            twitch_user_id: "owner".into(),
            clip_id: clip,
            job_id: None,
            mode: ProductionMode::Static,
            version_no: 1,
            parent_version_id: None,
            variant_index: 0,
        },
    )
    .await
    .expect("video");
    produced_video::complete_render(
        pool,
        "owner",
        video.id,
        &RenderedVideo {
            file_path: file.to_string_lossy().into_owned(),
            duration_ms: 15_000,
            needs_review: true,
            review_reasons: vec![],
            metadata: json!({"platforms": {"youtube": {"title": "Highlight", "hashtags": ["Deadlock"]}}}),
        },
    )
    .await
    .expect("render");
    (video.id, file)
}

async fn make_due(pool: &PgPool, id: i64) {
    sqlx::query("UPDATE social_produced_videos SET scheduled_at = clock_timestamp() - INTERVAL '1 minute' WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .expect("due video");
    sqlx::query("UPDATE twitch_clips_upload_queue SET scheduled_at = clock_timestamp() - INTERVAL '1 minute' WHERE produced_video_id = $1")
        .bind(id)
        .execute(pool)
        .await
        .expect("due destinations");
}

async fn destinations(pool: &PgPool, id: i64) -> Vec<(String, String, Option<String>)> {
    sqlx::query_as("SELECT platform, status, outcome_reason FROM twitch_clips_upload_queue WHERE produced_video_id = $1 ORDER BY platform")
        .bind(id)
        .fetch_all(pool)
        .await
        .expect("destinations")
}

#[derive(Default)]
struct ScriptedPublisher {
    script: Mutex<Vec<Result<PublishOutcome, UploadError>>>,
    calls: Mutex<Vec<(String, String)>>,
}

impl ScriptedPublisher {
    fn with(script: Vec<Result<PublishOutcome, UploadError>>) -> Arc<Self> {
        Arc::new(Self {
            script: Mutex::new(script),
            calls: Mutex::default(),
        })
    }

    fn calls(&self) -> Vec<(String, String)> {
        self.calls.lock().expect("calls").clone()
    }
}

#[async_trait]
impl DestinationPublisher for ScriptedPublisher {
    async fn publish(
        &self,
        _owner: &str,
        platform: &str,
        request: &PublishRequest,
    ) -> Result<PublishOutcome, UploadError> {
        self.calls
            .lock()
            .expect("calls")
            .push((platform.into(), request.file_path.clone()));
        let mut script = self.script.lock().expect("script");
        if script.is_empty() {
            return Err(UploadError::Api("unexpected upload".into()));
        }
        script.remove(0)
    }
}

fn published(id: &str) -> Result<PublishOutcome, UploadError> {
    Ok(PublishOutcome {
        external_id: id.into(),
        visibility: Some("public".into()),
    })
}

fn decisions(platforms: &[&str]) -> Vec<DestinationDecision> {
    platforms
        .iter()
        .map(|platform| DestinationDecision {
            platform: platform.to_string(),
            tiktok_options: None,
        })
        .collect()
}

async fn tiktok_choice(pool: &PgPool, id: i64) -> Value {
    let subject = tiktok_subject(pool, "owner", id)
        .await
        .expect("tiktok subject");
    json!({
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
    })
}

async fn music_consent(pool: &PgPool, account: &str) {
    sqlx::query("INSERT INTO social_media_settings (key, value) VALUES ($1, $2::text::jsonb) ON CONFLICT (key) DO NOTHING")
        .bind(format!("tiktok_music_consent:owner:{account}"))
        .bind(json!({"platform_user_id": account, "consented_at": "2026-10-01T00:00:00Z"}).to_string())
        .execute(pool)
        .await
        .expect("music consent");
}

async fn consented(pool: &PgPool, id: i64, platforms: &[&str]) -> Vec<DestinationDecision> {
    let mut decisions = decisions(platforms);
    for decision in decisions
        .iter_mut()
        .filter(|decision| decision.platform == "tiktok")
    {
        let choice = tiktok_choice(pool, id).await;
        if let Some(account) = choice["platform_user_id"].as_str() {
            music_consent(pool, account).await;
        }
        decision.tiktok_options = Some(choice);
    }
    decisions
}

fn with_tiktok(choice: Value) -> Vec<DestinationDecision> {
    vec![DestinationDecision {
        platform: "tiktok".into(),
        tiktok_options: Some(choice),
    }]
}

async fn rejection(pool: &PgPool, id: i64, decisions: &[DestinationDecision]) -> String {
    match approve(pool, "owner", id, decisions).await {
        Err(PublishError::Invalid(message)) => message,
        other => panic!("expected a precise TikTok rejection, got {other:?}"),
    }
}

async fn bound_options(pool: &PgPool, id: i64) -> Option<Value> {
    sqlx::query_scalar(
        "SELECT metadata #> '{publication,tiktok_options}' FROM social_produced_videos WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("bound options")
}

async fn approval_count(pool: &PgPool, id: i64) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM social_media_clip_approval WHERE produced_video_id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("approvals")
}

async fn tiktok_state(pool: &PgPool, id: i64) -> &'static str {
    snapshot(pool, "owner")
        .await
        .expect("snapshot")
        .videos
        .into_iter()
        .find(|video| video.id == id)
        .expect("listed video")
        .tiktok_consent
}

async fn save_choice_setting(pool: &PgPool, key: String, value: &Value) {
    sqlx::query("INSERT INTO social_media_settings (key, value) VALUES ($1, $2::text::jsonb)")
        .bind(key)
        .bind(value.to_string())
        .execute(pool)
        .await
        .expect("saved choices");
}

#[tokio::test]
async fn tiktok_consent_binds_the_exact_version_once_and_drafts_do_not_authorize() {
    let pool = pool().await;
    configure(&pool, &["tiktok"], &[("tiktok", "tt-1")]).await;
    let (id, file) = video(&pool, "consent").await;
    let clip: i64 = sqlx::query_scalar("SELECT clip_id FROM social_produced_videos WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("clip");
    let draft = json!({"caption": "Entwurf", "privacy_level": "SELF_ONLY", "allow_comment": false, "allow_duet": false, "allow_stitch": false});
    save_choice_setting(&pool, format!("tiktok_draft:owner:{clip}"), &draft).await;
    save_choice_setting(&pool, "tiktok_defaults:owner".into(), &draft).await;
    assert_eq!(
        rejection(&pool, id, &decisions(&["tiktok"])).await,
        TikTokBindingIssue::ConsentMissing.message()
    );
    assert_eq!(
        rejection(&pool, id, &with_tiktok(draft)).await,
        TIKTOK_OPTIONS_UNREADABLE
    );
    let mut unconfirmed = tiktok_choice(&pool, id).await;
    unconfirmed["consent"] = json!(false);
    assert_eq!(
        rejection(&pool, id, &with_tiktok(unconfirmed)).await,
        TikTokBindingIssue::ConsentMissing.message()
    );
    let choice = tiktok_choice(&pool, id).await;
    assert_eq!(
        rejection(&pool, id, &with_tiktok(choice.clone())).await,
        TikTokBindingIssue::MusicConsentMissing.message()
    );
    music_consent(&pool, "tt-1").await;
    let mut disclosed = choice.clone();
    disclosed["commercial_content"] = json!(true);
    assert_eq!(
        rejection(&pool, id, &with_tiktok(disclosed)).await,
        TikTokBindingIssue::OptionsInvalid.message()
    );
    assert_eq!(approval_count(&pool, id).await, 0);
    assert!(bound_options(&pool, id).await.is_none());
    assert!(destinations(&pool, id).await.is_empty());
    assert_eq!(tiktok_state(&pool, id).await, "missing");
    approve(&pool, "owner", id, &with_tiktok(choice))
        .await
        .expect("exact consent");
    let stored = bound_options(&pool, id).await.expect("stored binding");
    assert_eq!(stored["video_path"], json!(file.to_string_lossy()));
    assert_eq!(stored["caption"], json!("Highlight\n\n#Deadlock"));
    assert_eq!(tiktok_state(&pool, id).await, "bound");
    let subject = tiktok_subject(&pool, "owner", id).await.expect("subject");
    assert!(subject.binding().is_ok());
    approve(&pool, "owner", id, &decisions(&["tiktok"]))
        .await
        .expect("existing binding is reused");
    assert_eq!(bound_options(&pool, id).await, Some(stored));
    cleanup(pool).await;
}

#[tokio::test]
async fn tiktok_binding_is_invalidated_by_edits_account_changes_and_file_changes() {
    let pool = pool().await;
    configure(&pool, &["tiktok"], &[("tiktok", "tt-1")]).await;
    let (id, file) = video(&pool, "invalidated").await;
    let first = tiktok_choice(&pool, id).await;
    music_consent(&pool, "tt-1").await;
    approve(&pool, "owner", id, &with_tiktok(first.clone()))
        .await
        .expect("first consent");
    sqlx::query("UPDATE social_produced_videos SET status = 'review', expires_at = clock_timestamp() + INTERVAL '1 day' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("back in review");
    let mut copies = std::collections::BTreeMap::new();
    copies.insert(
        "youtube".to_string(),
        crate::ai_editor::metadata::PlatformCopy {
            title: "Neuer Titel".into(),
            description: String::new(),
            hashtags: vec!["Deadlock".into()],
        },
    );
    crate::ai_editor::metadata::save(&pool, "owner", id, copies)
        .await
        .expect("edited copy");
    assert!(bound_options(&pool, id).await.is_none());
    assert_eq!(tiktok_state(&pool, id).await, "missing");
    assert_eq!(
        rejection(&pool, id, &with_tiktok(first)).await,
        TikTokBindingIssue::CaptionChanged.message()
    );
    let second = tiktok_choice(&pool, id).await;
    assert_eq!(second["caption"], json!("Neuer Titel\n\n#Deadlock"));
    approve(&pool, "owner", id, &with_tiktok(second.clone()))
        .await
        .expect("consent for the edited version");
    let held = destinations(&pool, id).await;
    sqlx::query("INSERT INTO social_media_platform_auth (platform, twitch_user_id, platform_user_id, authorized_at) VALUES ('tiktok', 'owner', 'tt-2', '2999-01-01')")
        .execute(&pool)
        .await
        .expect("other account");
    music_consent(&pool, "tt-2").await;
    let subject = tiktok_subject(&pool, "owner", id).await.expect("subject");
    assert_eq!(
        subject.binding().err(),
        Some(TikTokBindingIssue::AccountChanged)
    );
    assert_eq!(tiktok_state(&pool, id).await, "stale");
    assert_eq!(
        rejection(&pool, id, &decisions(&["tiktok"])).await,
        TikTokBindingIssue::AccountChanged.message()
    );
    assert_eq!(
        rejection(&pool, id, &with_tiktok(second)).await,
        TikTokBindingIssue::AccountChanged.message()
    );
    assert_eq!(destinations(&pool, id).await, held);
    let third = tiktok_choice(&pool, id).await;
    approve(&pool, "owner", id, &with_tiktok(third.clone()))
        .await
        .expect("consent for the new account");
    std::fs::write(&file, b"different rendered bytes").expect("changed file");
    let subject = tiktok_subject(&pool, "owner", id).await.expect("subject");
    assert_eq!(
        subject.binding().err(),
        Some(TikTokBindingIssue::FileChanged)
    );
    assert_eq!(
        rejection(&pool, id, &with_tiktok(third)).await,
        TikTokBindingIssue::FileChanged.message()
    );
    let stored = bound_options(&pool, id).await.expect("old binding kept");
    assert_eq!(stored["platform_user_id"], json!("tt-2"));
    assert_ne!(
        stored["approved_video_sha256"],
        json!(subject.approved_video_sha256)
    );
    cleanup(pool).await;
}

#[tokio::test]
async fn tiktok_binding_cannot_change_after_the_effect_started() {
    let pool = pool().await;
    configure(&pool, &["tiktok"], &[("tiktok", "tt-1")]).await;
    let (id, _) = video(&pool, "started").await;
    let choice = tiktok_choice(&pool, id).await;
    music_consent(&pool, "tt-1").await;
    approve(&pool, "owner", id, &with_tiktok(choice.clone()))
        .await
        .expect("consent");
    sqlx::query("UPDATE twitch_clips_upload_queue SET effect_started_at = clock_timestamp() WHERE produced_video_id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("effect started");
    let mut public = choice;
    public["privacy_level"] = json!("PUBLIC_TO_EVERYONE");
    assert_eq!(
        rejection(&pool, id, &with_tiktok(public)).await,
        TIKTOK_EFFECT_STARTED
    );
    assert_eq!(
        bound_options(&pool, id).await.expect("binding")["privacy_level"],
        json!("SELF_ONLY")
    );
    cleanup(pool).await;
}

#[tokio::test]
async fn partial_approval_discards_rest_and_retries_only_failed_platform_with_exact_file() {
    let pool = pool().await;
    configure(
        &pool,
        &["youtube", "instagram", "tiktok"],
        &[("youtube", "yt-1"), ("instagram", "ig-1")],
    )
    .await;
    let (id, file) = video(&pool, "partial").await;
    approve(&pool, "owner", id, &decisions(&["youtube"]))
        .await
        .expect("approve youtube only");
    let snapshot = snapshot(&pool, "owner").await.expect("snapshot");
    let event = snapshot.videos[0].event.as_ref().expect("scheduled event");
    assert!(event.scheduled_at > chrono::Utc::now());
    assert_eq!(event.destinations.len(), 1);
    make_due(&pool, id).await;
    let publisher = ScriptedPublisher::with(vec![
        Err(UploadError::Api("temporary".into())),
        published("yt-video"),
    ]);
    let executor = PublishExecutor::new(pool.clone(), publisher.clone());
    executor.tick().await.expect("first tick");
    assert_eq!(
        destinations(&pool, id).await,
        vec![
            (
                "instagram".into(),
                queue_status::DISCARDED.into(),
                Some("not_approved_at_execution".into())
            ),
            (
                "tiktok".into(),
                queue_status::DISCARDED.into(),
                Some("not_approved_at_execution".into())
            ),
            (
                "youtube".into(),
                queue_status::RETRY.into(),
                Some("platform_error".into())
            ),
        ]
    );
    executor.tick().await.expect("idle tick");
    assert_eq!(publisher.calls().len(), 1);
    assert!(matches!(
        retry(&pool, "owner", id, "instagram").await,
        Err(PublishError::RetryBlocked)
    ));
    retry(&pool, "owner", id, "youtube")
        .await
        .expect("manual youtube retry");
    retry(&pool, "owner", id, "youtube")
        .await
        .expect("idempotent retry");
    executor.tick().await.expect("retry tick");
    let calls = publisher.calls();
    assert_eq!(calls.len(), 2);
    assert!(calls
        .iter()
        .all(|(platform, path)| platform == "youtube" && path == &file.to_string_lossy()));
    let (status, deleted): (String, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT status, artifact_deleted_at FROM social_produced_videos WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("video");
    assert_eq!(status, "published");
    assert!(deleted.is_some());
    assert!(!file.exists());
    let (uploaded, external): (bool, Option<String>) = sqlx::query_as(
        "SELECT c.uploaded_youtube, q.external_id FROM twitch_clips_upload_queue q JOIN twitch_clips_social_media c ON c.id = q.clip_id WHERE q.produced_video_id = $1 AND q.platform = 'youtube'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("published destination");
    assert!(uploaded);
    assert_eq!(external.as_deref(), Some("yt-video"));
    executor.tick().await.expect("settled tick");
    assert_eq!(publisher.calls().len(), 2);
    cleanup(pool).await;
}

#[tokio::test]
async fn ambiguous_request_is_never_uploaded_twice_and_changed_file_is_held() {
    let pool = pool().await;
    configure(
        &pool,
        &["youtube", "instagram"],
        &[("youtube", "yt-1"), ("instagram", "ig-1")],
    )
    .await;
    let (id, file) = video(&pool, "ambiguous").await;
    approve(&pool, "owner", id, &decisions(&["youtube", "instagram"]))
        .await
        .expect("approve");
    std::fs::write(&file, b"exact rendered bytes").expect("same bytes");
    make_due(&pool, id).await;
    let publisher = ScriptedPublisher::with(vec![Err(UploadError::Request("timeout".into()))]);
    let executor = PublishExecutor::new(pool.clone(), publisher.clone());
    executor.tick().await.expect("first tick");
    let states = destinations(&pool, id).await;
    let uncertain = states
        .iter()
        .filter(|row| row.1 == queue_status::UNCERTAIN)
        .count();
    assert_eq!(uncertain, 1);
    assert_eq!(publisher.calls().len(), 2);
    executor.tick().await.expect("second tick");
    executor.tick().await.expect("third tick");
    assert_eq!(publisher.calls().len(), 2);
    assert!(file.exists());
    let (id, file) = video(&pool, "changed").await;
    approve(&pool, "owner", id, &decisions(&["youtube"]))
        .await
        .expect("approve");
    std::fs::write(&file, b"different bytes").expect("replace");
    make_due(&pool, id).await;
    executor.tick().await.expect("changed tick");
    assert_eq!(
        destinations(&pool, id).await,
        vec![
            (
                "instagram".into(),
                queue_status::DISCARDED.into(),
                Some("not_approved_at_execution".into())
            ),
            (
                "youtube".into(),
                queue_status::HELD.into(),
                Some("exact_file_changed".into())
            ),
        ]
    );
    assert_eq!(publisher.calls().len(), 2);
    assert!(matches!(
        retry(&pool, "owner", id, "youtube").await,
        Err(PublishError::RetryBlocked)
    ));
    cleanup(pool).await;
}

#[tokio::test]
async fn unapproved_review_expires_after_retention_and_releases_slot() {
    let pool = pool().await;
    configure(&pool, &["youtube"], &[("youtube", "yt-1")]).await;
    let (id, file) = video(&pool, "expiry").await;
    sqlx::query("UPDATE social_produced_videos SET expires_at = clock_timestamp() - INTERVAL '1 second' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("expired");
    let executor = PublishExecutor::new(pool.clone(), ScriptedPublisher::with(vec![]));
    let report = executor.tick().await.expect("tick");
    assert_eq!(report.expired, 1);
    let (status, deleted): (String, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT status, artifact_deleted_at FROM social_produced_videos WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("video");
    assert_eq!(status, "expired");
    assert!(deleted.is_some());
    assert!(!file.exists());
    let view = snapshot(&pool, "owner").await.expect("snapshot");
    assert_eq!(view.videos[0].artifact_state, "deleted");
    cleanup(pool).await;
}

#[tokio::test]
async fn due_event_without_approval_releases_slot_and_paused_owner_is_skipped() {
    let pool = pool().await;
    configure(&pool, &["youtube"], &[("youtube", "yt-1")]).await;
    let (id, _) = video(&pool, "unapproved").await;
    sqlx::query("UPDATE social_produced_videos SET scheduled_at = clock_timestamp() - INTERVAL '1 minute' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("slot");
    let publisher = ScriptedPublisher::with(vec![published("never")]);
    let executor = PublishExecutor::new(pool.clone(), publisher.clone());
    executor.tick().await.expect("tick");
    let (status, slot): (String, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT status, scheduled_at FROM social_produced_videos WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("video");
    assert_eq!(status, "review");
    assert!(slot.is_none());
    let (paused_id, _) = video(&pool, "paused").await;
    approve(&pool, "owner", paused_id, &decisions(&["youtube"]))
        .await
        .expect("approve");
    make_due(&pool, paused_id).await;
    set_paused(&pool, "owner", true).await.expect("pause");
    executor.tick().await.expect("paused tick");
    assert!(publisher.calls().is_empty());
    set_paused(&pool, "owner", false).await.expect("resume");
    executor.tick().await.expect("resumed tick");
    assert_eq!(publisher.calls().len(), 1);
    cleanup(pool).await;
}

#[tokio::test]
async fn late_event_is_moved_instead_of_published_in_a_burst() {
    let pool = pool().await;
    configure(&pool, &["youtube"], &[("youtube", "yt-1")]).await;
    let (id, _) = video(&pool, "late").await;
    approve(&pool, "owner", id, &decisions(&["youtube"]))
        .await
        .expect("approve");
    sqlx::query("UPDATE social_produced_videos SET scheduled_at = clock_timestamp() - INTERVAL '3 hours' WHERE id = $1")
        .bind(id).execute(&pool).await.expect("late video");
    sqlx::query("UPDATE twitch_clips_upload_queue SET scheduled_at = clock_timestamp() - INTERVAL '3 hours' WHERE produced_video_id = $1")
        .bind(id).execute(&pool).await.expect("late destination");
    let publisher = ScriptedPublisher::with(vec![]);
    let report = PublishExecutor::new(pool.clone(), publisher.clone())
        .tick()
        .await
        .expect("tick");
    assert_eq!(report.rescheduled, 1);
    assert!(publisher.calls().is_empty());
    let slot: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT scheduled_at FROM social_produced_videos WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("slot");
    assert!(slot > chrono::Utc::now());
    cleanup(pool).await;
}

#[tokio::test]
async fn mode_switch_requires_explicit_pending_choice_and_review_revokes_automatic_approval() {
    let pool = pool().await;
    configure(&pool, &["youtube"], &[("youtube", "yt-1")]).await;
    let (id, _) = video(&pool, "manual").await;
    let unconfirmed = ModeTransition {
        mode: PublishMode::FullAuto,
        confirmed: false,
        pending: Some(PendingHandling::TransferEligible),
    };
    assert!(matches!(
        transition_mode(&pool, "owner", &unconfirmed).await,
        Err(PublishError::Confirmation)
    ));
    let missing = ModeTransition {
        mode: PublishMode::FullAuto,
        confirmed: true,
        pending: None,
    };
    assert!(matches!(
        transition_mode(&pool, "owner", &missing).await,
        Err(PublishError::PendingChoice)
    ));
    transition_mode(
        &pool,
        "owner",
        &ModeTransition {
            mode: PublishMode::FullAuto,
            confirmed: true,
            pending: Some(PendingHandling::TransferEligible),
        },
    )
    .await
    .expect("full auto");
    let needs_review: bool =
        sqlx::query_scalar("SELECT needs_review FROM social_produced_videos WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("manual video");
    assert!(needs_review, "manual videos are never transferred");
    sqlx::query("INSERT INTO social_media_clip_approval (produced_video_id, platform, state, approved_platforms, approver_user_id, decided_at) VALUES ($1, 'youtube', 'approved', '[]', 'autopilot', NOW())")
        .bind(id)
        .execute(&pool)
        .await
        .expect("automatic approval");
    sqlx::query(
        "UPDATE social_produced_videos SET status = 'approved', needs_review = FALSE WHERE id = $1",
    )
    .bind(id)
    .execute(&pool)
    .await
    .expect("approved");
    transition_mode(
        &pool,
        "owner",
        &ModeTransition {
            mode: PublishMode::Review,
            confirmed: true,
            pending: None,
        },
    )
    .await
    .expect("review");
    let (status, needs_review, approvals): (String, bool, i64) = sqlx::query_as(
        "SELECT status, needs_review, (SELECT COUNT(*) FROM social_media_clip_approval WHERE produced_video_id = $1) FROM social_produced_videos WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("revoked");
    assert_eq!(
        (status.as_str(), needs_review, approvals),
        ("review", true, 0)
    );
    cleanup(pool).await;
}

struct RecordingNotifier(Mutex<Vec<(String, String)>>);

#[async_trait]
impl AutopilotNotifier for RecordingNotifier {
    async fn send(&self, owner: &str, content: &str) -> DiscordDelivery {
        self.0
            .lock()
            .expect("messages")
            .push((owner.into(), content.into()));
        DiscordDelivery::Sent
    }
}

#[tokio::test]
async fn notifications_respect_preferences_and_are_sent_once_with_dashboard_link() {
    let pool = pool().await;
    configure(&pool, &["youtube"], &[("youtube", "yt-1")]).await;
    let (_, _) = video(&pool, "silent").await;
    let notifier = Arc::new(RecordingNotifier(Mutex::default()));
    let executor = PublishExecutor::new(pool.clone(), ScriptedPublisher::with(vec![]))
        .with_notifier(notifier.clone());
    executor.tick().await.expect("dashboard only tick");
    assert!(notifier.0.lock().expect("messages").is_empty());
    let view = snapshot(&pool, "owner").await.expect("snapshot");
    assert_eq!(view.notifications.len(), 1);
    assert_eq!(view.notifications[0].kind, "ready_for_review");
    assert_eq!(view.notifications[0].discord_state, "disabled");
    let mut preferences = view.settings.notifications.clone();
    preferences.discord = true;
    preferences.email = true;
    let saved = save_notifications(&pool, "owner", &preferences)
        .await
        .expect("save");
    assert!(!saved.email);
    let (_, _) = video(&pool, "discord").await;
    executor.tick().await.expect("discord tick");
    executor.tick().await.expect("dedupe tick");
    let messages = notifier.0.lock().expect("messages").clone();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].1.ends_with(DASHBOARD_LINK));
    cleanup(pool).await;
}

async fn artifact(pool: &PgPool, id: i64) -> Option<chrono::DateTime<chrono::Utc>> {
    sqlx::query_scalar("SELECT artifact_deleted_at FROM social_produced_videos WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("artifact state")
}

#[tokio::test]
async fn open_destination_keeps_file_past_retention_and_deleted_file_is_never_retried() {
    let pool = pool().await;
    configure(
        &pool,
        &["youtube", "tiktok"],
        &[("youtube", "yt-1"), ("tiktok", "tt-1")],
    )
    .await;
    let (id, file) = video(&pool, "open").await;
    approve(
        &pool,
        "owner",
        id,
        &consented(&pool, id, &["youtube", "tiktok"]).await,
    )
    .await
    .expect("approve");
    make_due(&pool, id).await;
    let publisher = ScriptedPublisher::with(vec![published("yt-video")]);
    let executor = PublishExecutor::new(pool.clone(), publisher.clone());
    executor.tick().await.expect("publish tick");
    assert_eq!(
        destinations(&pool, id).await,
        vec![
            (
                "tiktok".into(),
                queue_status::HELD.into(),
                Some("tiktok_effect_boundary_pending".into())
            ),
            ("youtube".into(), queue_status::PUBLISHED.into(), None),
        ]
    );
    sqlx::query("UPDATE social_produced_videos SET publication_executed_at = clock_timestamp() - INTERVAL '8 days' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("old publication");
    executor.tick().await.expect("held tick");
    assert!(artifact(&pool, id).await.is_none());
    assert!(file.exists());
    for status in [queue_status::FAILED, queue_status::UNCERTAIN] {
        sqlx::query("UPDATE twitch_clips_upload_queue SET status = $2 WHERE produced_video_id = $1 AND platform = 'tiktok'")
            .bind(id)
            .bind(status)
            .execute(&pool)
            .await
            .expect("open destination");
        executor.tick().await.expect("open tick");
        assert!(artifact(&pool, id).await.is_none());
        assert!(file.exists());
    }
    sqlx::query("UPDATE twitch_clips_upload_queue SET status = $2, outcome_reason = 'platform_error', next_retry_at = clock_timestamp() + INTERVAL '1 hour' WHERE produced_video_id = $1 AND platform = 'tiktok'")
        .bind(id)
        .bind(queue_status::RETRY)
        .execute(&pool)
        .await
        .expect("retryable destination");
    sqlx::query(
        "UPDATE social_produced_videos SET artifact_deleted_at = clock_timestamp() WHERE id = $1",
    )
    .bind(id)
    .execute(&pool)
    .await
    .expect("deleted artifact");
    std::fs::remove_file(&file).expect("remove file");
    assert!(matches!(
        retry(&pool, "owner", id, "tiktok").await,
        Err(PublishError::ArtifactDeleted)
    ));
    sqlx::query("UPDATE twitch_clips_upload_queue SET next_retry_at = clock_timestamp() - INTERVAL '1 minute' WHERE produced_video_id = $1 AND platform = 'tiktok'")
        .bind(id)
        .execute(&pool)
        .await
        .expect("due retry");
    executor.tick().await.expect("deleted tick");
    assert_eq!(publisher.calls().len(), 1);
    let status: String = sqlx::query_scalar(
        "SELECT status FROM twitch_clips_upload_queue WHERE produced_video_id = $1 AND platform = 'tiktok'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("tiktok destination");
    assert_eq!(status, queue_status::RETRY);
    cleanup(pool).await;
}

#[tokio::test]
async fn review_switch_holds_pending_destination_of_executed_event() {
    let pool = pool().await;
    configure(
        &pool,
        &["youtube", "instagram"],
        &[("youtube", "yt-1"), ("instagram", "ig-1")],
    )
    .await;
    let (id, file) = video(&pool, "executed").await;
    approve(&pool, "owner", id, &decisions(&["youtube"]))
        .await
        .expect("approve youtube");
    transition_mode(
        &pool,
        "owner",
        &ModeTransition {
            mode: PublishMode::FullAuto,
            confirmed: true,
            pending: Some(PendingHandling::KeepReview),
        },
    )
    .await
    .expect("full auto");
    sqlx::query("INSERT INTO social_media_clip_approval (produced_video_id, platform, state, approved_platforms, approver_user_id, decided_at) VALUES ($1, 'instagram', 'approved', '[]', 'autopilot', NOW())")
        .bind(id)
        .execute(&pool)
        .await
        .expect("automatic approval");
    sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id, produced_video_id, platform, status, scheduled_at) SELECT clip_id, id, 'instagram', $2, scheduled_at FROM social_produced_videos WHERE id = $1")
        .bind(id)
        .bind(queue_status::PENDING)
        .execute(&pool)
        .await
        .expect("pending instagram");
    sqlx::query("UPDATE twitch_clips_upload_queue SET status = $2 WHERE produced_video_id = $1 AND platform = 'youtube'")
        .bind(id)
        .bind(queue_status::PUBLISHED)
        .execute(&pool)
        .await
        .expect("published youtube");
    sqlx::query("UPDATE social_produced_videos SET publication_executed_at = clock_timestamp() WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("executed");
    make_due(&pool, id).await;
    transition_mode(
        &pool,
        "owner",
        &ModeTransition {
            mode: PublishMode::Review,
            confirmed: true,
            pending: None,
        },
    )
    .await
    .expect("review");
    assert_eq!(
        destinations(&pool, id).await,
        vec![
            (
                "instagram".into(),
                queue_status::HELD.into(),
                Some("review_required".into())
            ),
            ("youtube".into(), queue_status::PUBLISHED.into(), None),
        ]
    );
    let publisher = ScriptedPublisher::with(vec![published("ig-video")]);
    let executor = PublishExecutor::new(pool.clone(), publisher.clone());
    executor.tick().await.expect("held tick");
    assert!(publisher.calls().is_empty());
    assert!(file.exists());
    approve(&pool, "owner", id, &decisions(&["instagram"]))
        .await
        .expect("explicit instagram approval");
    executor.tick().await.expect("approved tick");
    assert_eq!(publisher.calls().len(), 1);
    cleanup(pool).await;
}

#[tokio::test]
async fn approved_video_without_slot_is_scheduled_again() {
    let pool = pool().await;
    configure(&pool, &["youtube"], &[("youtube", "yt-1")]).await;
    let (id, _) = video(&pool, "slotless").await;
    approve(&pool, "owner", id, &decisions(&["youtube"]))
        .await
        .expect("approve");
    let mut tx = pool.begin().await.expect("tx");
    release_slot(&mut tx, id).await.expect("release");
    tx.commit().await.expect("commit");
    let (status, slot): (String, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT status, scheduled_at FROM social_produced_videos WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("released");
    assert_eq!((status.as_str(), slot), ("approved", None));
    PublishExecutor::new(pool.clone(), ScriptedPublisher::with(vec![]))
        .tick()
        .await
        .expect("tick");
    let (status, slot): (String, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT status, scheduled_at FROM social_produced_videos WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("rescheduled");
    assert_eq!(status, "scheduled");
    assert!(slot.is_some_and(|slot| slot > chrono::Utc::now()));
    cleanup(pool).await;
}

#[tokio::test]
async fn pending_discord_message_is_dropped_after_opt_out() {
    let pool = pool().await;
    configure(&pool, &["youtube"], &[("youtube", "yt-1")]).await;
    let mut preferences = autopilot_settings::load(&pool, "owner")
        .await
        .expect("settings")
        .notifications;
    preferences.discord = true;
    save_notifications(&pool, "owner", &preferences)
        .await
        .expect("enable discord");
    let (_, _) = video(&pool, "opt-out").await;
    let silent = PublishExecutor::new(pool.clone(), ScriptedPublisher::with(vec![]));
    silent.tick().await.expect("collect tick");
    let pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM social_autopilot_notifications WHERE discord_state = 'pending'",
    )
    .fetch_one(&pool)
    .await
    .expect("pending");
    assert_eq!(pending, 1);
    preferences.discord = false;
    save_notifications(&pool, "owner", &preferences)
        .await
        .expect("disable discord");
    let notifier = Arc::new(RecordingNotifier(Mutex::default()));
    PublishExecutor::new(pool.clone(), ScriptedPublisher::with(vec![]))
        .with_notifier(notifier.clone())
        .tick()
        .await
        .expect("dispatch tick");
    assert!(notifier.0.lock().expect("messages").is_empty());
    let state: String =
        sqlx::query_scalar("SELECT discord_state FROM social_autopilot_notifications")
            .fetch_one(&pool)
            .await
            .expect("state");
    assert_eq!(state, "disabled");
    cleanup(pool).await;
}

async fn queue_state(pool: &PgPool, id: i64, platform: &str) -> (String, Option<String>) {
    sqlx::query_as("SELECT status, outcome_reason FROM twitch_clips_upload_queue WHERE produced_video_id = $1 AND platform = $2")
        .bind(id)
        .bind(platform)
        .fetch_one(pool)
        .await
        .expect("destination state")
}

#[tokio::test]
async fn stale_claim_cannot_change_a_recovered_destination() {
    let pool = pool().await;
    configure(&pool, &["youtube"], &[("youtube", "yt-1")]).await;
    let (id, _) = video(&pool, "fenced").await;
    approve(&pool, "owner", id, &decisions(&["youtube"]))
        .await
        .expect("approve");
    make_due(&pool, id).await;
    sqlx::query("UPDATE social_produced_videos SET publication_executed_at = clock_timestamp() WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("executed");
    let publisher = ScriptedPublisher::with(vec![published("yt-video")]);
    let executor = PublishExecutor::new(pool.clone(), publisher.clone());
    let stale = executor.claim().await.expect("claim").expect("first claim");
    sqlx::query("UPDATE twitch_clips_upload_queue SET last_attempt_at = clock_timestamp() - INTERVAL '11 minutes' WHERE produced_video_id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("stale claim");
    executor.recover_stale().await.expect("recover");
    let fresh = executor
        .claim()
        .await
        .expect("claim")
        .expect("second claim");
    executor
        .settle(&stale, queue_status::HELD, Some("exact_file_changed"), None)
        .await
        .expect("stale hold");
    assert!(!executor
        .start_effect(&stale, Some("yt-1"), None)
        .await
        .expect("stale start"));
    executor
        .complete(
            &stale,
            &PublishOutcome {
                external_id: "stale".into(),
                visibility: None,
            },
        )
        .await
        .expect("stale complete");
    assert_eq!(
        queue_state(&pool, id, "youtube").await,
        (queue_status::PREPARING.into(), None)
    );
    executor.process(fresh).await.expect("fresh upload");
    assert_eq!(
        queue_state(&pool, id, "youtube").await,
        (queue_status::PUBLISHED.into(), None)
    );
    assert_eq!(publisher.calls().len(), 1);
    cleanup(pool).await;
}

#[tokio::test]
async fn rebinding_one_platform_keeps_the_hash_other_destinations_were_approved_for() {
    let pool = pool().await;
    configure(
        &pool,
        &["youtube", "instagram"],
        &[("youtube", "yt-1"), ("instagram", "ig-1")],
    )
    .await;
    let (id, file) = video(&pool, "rebound").await;
    approve(&pool, "owner", id, &decisions(&["youtube"]))
        .await
        .expect("approve youtube");
    std::fs::write(&file, b"different bytes").expect("replace");
    approve(&pool, "owner", id, &decisions(&["instagram"]))
        .await
        .expect("approve instagram");
    make_due(&pool, id).await;
    let publisher = ScriptedPublisher::with(vec![published("ig-video")]);
    PublishExecutor::new(pool.clone(), publisher.clone())
        .tick()
        .await
        .expect("tick");
    assert_eq!(
        destinations(&pool, id).await,
        vec![
            ("instagram".into(), queue_status::PUBLISHED.into(), None),
            (
                "youtube".into(),
                queue_status::HELD.into(),
                Some("exact_file_changed".into())
            ),
        ]
    );
    assert_eq!(
        publisher.calls(),
        vec![("instagram".to_string(), file.to_string_lossy().into_owned())]
    );
    cleanup(pool).await;
}

#[tokio::test]
async fn unschedulable_videos_do_not_starve_later_ones() {
    let pool = pool().await;
    configure(&pool, &["youtube"], &[("youtube", "yt-1")]).await;
    for index in 0..20 {
        let (id, _) = video(&pool, &format!("stuck-{index}")).await;
        sqlx::query("UPDATE social_produced_videos SET status = 'approved', metadata = metadata || '{\"publication\":{}}'::jsonb WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await
            .expect("stuck video");
        sqlx::query("INSERT INTO social_media_clip_approval (produced_video_id, platform, state, approved_platforms, approver_user_id, decided_at) VALUES ($1, 'youtube', 'approved', '[]', 'stranger', NOW())")
            .bind(id)
            .execute(&pool)
            .await
            .expect("foreign approval");
    }
    let (id, _) = video(&pool, "waiting").await;
    approve(&pool, "owner", id, &decisions(&["youtube"]))
        .await
        .expect("approve");
    let mut tx = pool.begin().await.expect("tx");
    release_slot(&mut tx, id).await.expect("release");
    tx.commit().await.expect("commit");
    let executor = PublishExecutor::new(pool.clone(), ScriptedPublisher::with(vec![]));
    executor.tick().await.expect("first tick");
    executor.tick().await.expect("second tick");
    let slot: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT scheduled_at FROM social_produced_videos WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("slot");
    assert!(slot.is_some());
    let deferred: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM social_produced_videos WHERE autopilot_deferred_stage = 'schedule' AND autopilot_deferred_until > clock_timestamp()",
    )
    .fetch_one(&pool)
    .await
    .expect("deferred");
    assert_eq!(deferred, 20);
    cleanup(pool).await;
}

#[tokio::test]
async fn snapshot_keeps_old_unresolved_work_visible() {
    let pool = pool().await;
    configure(&pool, &["youtube"], &[("youtube", "yt-1")]).await;
    let (open, _) = video(&pool, "old-review").await;
    for index in 0..55 {
        let (id, _) = video(&pool, &format!("finished-{index}")).await;
        sqlx::query("UPDATE social_produced_videos SET status = 'expired' WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await
            .expect("finished video");
    }
    let view = snapshot(&pool, "owner").await.expect("snapshot");
    assert_eq!(view.videos.len(), 51);
    assert!(view.videos.iter().any(|video| video.id == open));
    cleanup(pool).await;
}

async fn executed(
    pool: &PgPool,
    name: &str,
    offset: i32,
    statuses: &[(&str, &str)],
) -> (i64, std::path::PathBuf) {
    let (id, file) = video(pool, name).await;
    sqlx::query("UPDATE social_produced_videos SET status = 'approved', needs_review = FALSE, publication_executed_at = clock_timestamp() - INTERVAL '8 days' WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .expect("executed video");
    for (platform, status) in statuses {
        sqlx::query(
            "INSERT INTO twitch_clips_upload_queue (clip_id, produced_video_id, platform, status, scheduled_at, last_attempt_at, next_retry_at)
             SELECT v.clip_id, v.id, $2, $3, clock_timestamp() - INTERVAL '8 days' - make_interval(mins => $4),
                 CASE WHEN $3 = $6 THEN clock_timestamp() ELSE clock_timestamp() - INTERVAL '8 days' END,
                 CASE WHEN $3 = $5 THEN clock_timestamp() + INTERVAL '1 hour' END
             FROM social_produced_videos v WHERE v.id = $1",
        )
        .bind(id)
        .bind(platform)
        .bind(status)
        .bind(offset)
        .bind(queue_status::RETRY)
        .bind(queue_status::PREPARING)
        .execute(pool)
        .await
        .expect("destination");
    }
    (id, file)
}

async fn set_retention_start(pool: &PgPool, id: i64, days: i32) {
    sqlx::query("UPDATE social_produced_videos SET metadata = jsonb_set(metadata, '{artifact_retention}', jsonb_build_object('unresolved_since', clock_timestamp() - make_interval(days => $2))) WHERE id = $1")
        .bind(id)
        .bind(days)
        .execute(pool)
        .await
        .expect("retention start");
}

async fn retention(pool: &PgPool, id: i64) -> Option<serde_json::Value> {
    sqlx::query_scalar(
        "SELECT metadata -> 'artifact_retention' FROM social_produced_videos WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("retention")
}

#[tokio::test]
async fn unresolved_destinations_release_file_only_after_grace_period() {
    let pool = pool().await;
    configure(&pool, &["youtube", "tiktok"], &[("youtube", "yt-1")]).await;
    let publisher = ScriptedPublisher::with(vec![]);
    let executor = PublishExecutor::new(pool.clone(), publisher.clone());
    let expiring: [(&str, &[(&str, &str)]); 4] = [
        ("failed", &[("youtube", queue_status::FAILED)]),
        ("uncertain", &[("youtube", queue_status::UNCERTAIN)]),
        ("held", &[("youtube", queue_status::HELD)]),
        (
            "mixed",
            &[
                ("youtube", queue_status::PUBLISHED),
                ("tiktok", queue_status::FAILED),
            ],
        ),
    ];
    let keeping: [(&str, &[(&str, &str)]); 6] = [
        (
            "uncertain_tiktok",
            &[
                ("youtube", queue_status::PUBLISHED),
                ("tiktok", queue_status::UNCERTAIN),
            ],
        ),
        (
            "processing",
            &[
                ("youtube", queue_status::PUBLISHED),
                ("tiktok", queue_status::PROCESSING),
            ],
        ),
        ("retry", &[("youtube", queue_status::RETRY)]),
        ("preparing", &[("youtube", queue_status::PREPARING)]),
        (
            "effect",
            &[
                ("youtube", queue_status::FAILED),
                ("tiktok", queue_status::EFFECT_STARTED),
            ],
        ),
        (
            "waiting",
            &[
                ("youtube", queue_status::HELD),
                ("tiktok", queue_status::RETRY),
            ],
        ),
    ];
    let mut offset = 0;
    let mut expired = Vec::new();
    for (name, statuses) in expiring {
        offset += 1;
        let (id, file) = executed(&pool, name, offset, statuses).await;
        expired.push((id, file, statuses.len()));
    }
    let mut kept = Vec::new();
    for (name, statuses) in keeping {
        offset += 1;
        kept.push(executed(&pool, name, offset, statuses).await);
    }
    let (recent, recent_file) =
        executed(&pool, "recent", 99, &[("youtube", queue_status::UNCERTAIN)]).await;
    sqlx::query("UPDATE twitch_clips_upload_queue SET last_attempt_at = clock_timestamp() - INTERVAL '2 days' WHERE produced_video_id = $1")
        .bind(recent)
        .execute(&pool)
        .await
        .expect("recent attempt");
    executor.tick().await.expect("marking tick");
    for (id, file, _) in &expired {
        assert!(artifact(&pool, *id).await.is_none());
        assert!(file.exists());
        assert!(retention(&pool, *id).await.is_some());
        set_retention_start(&pool, *id, 8).await;
    }
    for (id, _) in &kept {
        assert!(retention(&pool, *id).await.is_none());
        set_retention_start(&pool, *id, 30).await;
    }
    set_retention_start(&pool, recent, 8).await;
    executor.tick().await.expect("cleanup tick");
    for (id, file, count) in &expired {
        assert!(artifact(&pool, *id).await.is_some());
        assert!(!file.exists());
        let record = retention(&pool, *id).await.expect("cleanup record");
        assert_eq!(record["reason"], "unresolved_destinations_expired");
        assert_eq!(
            record["destinations"].as_object().map(|map| map.len()),
            Some(*count)
        );
        let rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE produced_video_id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("kept destinations");
        assert_eq!(rows, *count as i64);
    }
    for (id, file) in &kept {
        assert!(artifact(&pool, *id).await.is_none());
        assert!(file.exists());
        assert!(retention(&pool, *id).await.is_none());
    }
    assert!(artifact(&pool, recent).await.is_none());
    assert!(recent_file.exists());
    executor.tick().await.expect("settled tick");
    assert!(publisher.calls().is_empty());
    cleanup(pool).await;
}

#[tokio::test]
async fn per_platform_schedule_publishes_each_destination_at_its_own_slot() {
    let pool = pool().await;
    configure(
        &pool,
        &["youtube", "tiktok"],
        &[("youtube", "yt-1"), ("tiktok", "tt-1")],
    )
    .await;
    let morning: Vec<serde_json::Value> = (1..=7)
        .map(|weekday| json!({"weekday": weekday, "local_time": "06:00"}))
        .collect();
    let evening: Vec<serde_json::Value> = (1..=7)
        .map(|weekday| json!({"weekday": weekday, "local_time": "18:00"}))
        .collect();
    sqlx::query("UPDATE social_media_autopilot_settings SET pipeline_mode = 'upload', schedule_scope = 'per_platform',
        platform_schedule_slots = $2::text::jsonb WHERE twitch_user_id = $1")
        .bind("owner")
        .bind(json!({"youtube": morning, "tiktok": evening}).to_string())
        .execute(&pool)
        .await
        .expect("per destination schedule");
    let (id, _) = video(&pool, "split").await;
    approve(
        &pool,
        "owner",
        id,
        &consented(&pool, id, &["youtube", "tiktok"]).await,
    )
    .await
    .expect("approve");
    let times: Vec<(String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT platform, scheduled_at FROM twitch_clips_upload_queue WHERE produced_video_id = $1 ORDER BY platform",
    )
    .bind(id)
    .fetch_all(&pool)
    .await
    .expect("destination times");
    assert_eq!(times.len(), 2);
    let gap = (times[0].1 - times[1].1).num_hours().abs();
    assert_eq!(gap, 12);
    let reference: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT scheduled_at FROM social_produced_videos WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("video slot");
    assert_eq!(reference, times[0].1.min(times[1].1));
    sqlx::query("UPDATE social_produced_videos SET scheduled_at = clock_timestamp() - INTERVAL '1 minute' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("due video");
    sqlx::query("UPDATE twitch_clips_upload_queue SET scheduled_at = clock_timestamp() - INTERVAL '1 minute' WHERE produced_video_id = $1 AND platform = 'youtube'")
        .bind(id)
        .execute(&pool)
        .await
        .expect("due youtube");
    let publisher = ScriptedPublisher::with(vec![published("yt-video")]);
    PublishExecutor::new(pool.clone(), publisher.clone())
        .tick()
        .await
        .expect("tick");
    let calls: Vec<String> = publisher
        .calls()
        .into_iter()
        .map(|(platform, _)| platform)
        .collect();
    assert_eq!(calls, vec!["youtube".to_string()]);
    let tiktok: (String, chrono::DateTime<chrono::Utc>) = sqlx::query_as(
        "SELECT status, scheduled_at FROM twitch_clips_upload_queue WHERE produced_video_id = $1 AND platform = 'tiktok'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("tiktok destination");
    assert!(tiktok.1 > chrono::Utc::now());
    assert_ne!(tiktok.0, "uploaded");
    cleanup(pool).await;
}

async fn split_schedule(pool: &PgPool, distribution: &str) {
    configure(
        pool,
        &["youtube", "tiktok"],
        &[("youtube", "yt-1"), ("tiktok", "tt-1")],
    )
    .await;
    let morning: Vec<serde_json::Value> = (1..=7)
        .map(|weekday| json!({"weekday": weekday, "local_time": "06:00"}))
        .collect();
    let evening: Vec<serde_json::Value> = (1..=7)
        .map(|weekday| json!({"weekday": weekday, "local_time": "18:00"}))
        .collect();
    sqlx::query("UPDATE social_media_autopilot_settings SET pipeline_mode = 'upload', schedule_scope = 'per_platform',
        platform_schedule_slots = $2::text::jsonb, distribution_policy = $3 WHERE twitch_user_id = $1")
        .bind("owner")
        .bind(json!({"youtube": morning, "tiktok": evening}).to_string())
        .bind(distribution)
        .execute(pool)
        .await
        .expect("per destination schedule");
}

async fn job_video(pool: &PgPool, name: &str) -> i64 {
    let (id, _) = video(pool, name).await;
    let job: i64 = sqlx::query_scalar(
        "INSERT INTO social_production_jobs (twitch_user_id, clip_id, queue_order, status, mode, started_at, finished_at)
        SELECT twitch_user_id, clip_id, 1, 'completed', 'static', NOW(), NOW() FROM social_produced_videos WHERE id = $1
        RETURNING id",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("completed job");
    sqlx::query("UPDATE social_produced_videos SET job_id = $2 WHERE id = $1")
        .bind(id)
        .bind(job)
        .execute(pool)
        .await
        .expect("video job");
    id
}

type Slots = Vec<(String, chrono::DateTime<chrono::Utc>)>;

async fn video_slots(pool: &PgPool, id: i64) -> (Slots, Slots) {
    let reserved = sqlx::query_as(
        "SELECT r.platform, r.scheduled_at FROM social_publication_reservations r
        JOIN social_produced_videos v ON v.job_id = r.job_id
        WHERE v.id = $1 AND r.state = 'reserved' ORDER BY r.platform",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .expect("reservations");
    let uploads = sqlx::query_as(
        "SELECT platform, scheduled_at FROM twitch_clips_upload_queue WHERE produced_video_id = $1 ORDER BY platform",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .expect("uploads");
    (reserved, uploads)
}

#[tokio::test]
async fn destination_without_own_slots_keeps_the_other_placements() {
    let pool = pool().await;
    split_schedule(&pool, "same_clip").await;
    let first = job_video(&pool, "first").await;
    approve(
        &pool,
        "owner",
        first,
        &consented(&pool, first, &["youtube", "tiktok"]).await,
    )
    .await
    .expect("approve first");
    let (reserved, uploads) = video_slots(&pool, first).await;
    assert_eq!(reserved, uploads);
    assert_eq!(reserved.len(), 2);
    let youtube = reserved[1].1;
    let mut policy = autopilot_settings::load_policy(&pool, "owner")
        .await
        .expect("policy");
    policy
        .platform_schedule_slots
        .insert("tiktok".into(), Vec::new());
    autopilot_settings::save_policy(&pool, "owner", &policy)
        .await
        .expect("tiktok without slots");
    let (reserved, uploads) = video_slots(&pool, first).await;
    assert_eq!(reserved, vec![("youtube".to_string(), youtube)]);
    assert_eq!(
        uploads,
        vec![
            ("tiktok".to_string(), youtube),
            ("youtube".to_string(), youtube)
        ]
    );
    let second = job_video(&pool, "second").await;
    approve(
        &pool,
        "owner",
        second,
        &consented(&pool, second, &["youtube", "tiktok"]).await,
    )
    .await
    .expect("approve without tiktok slots");
    let (reserved, uploads) = video_slots(&pool, second).await;
    assert_eq!(reserved.len(), 1);
    assert_eq!(reserved[0].0, "youtube");
    assert!(reserved[0].1 > youtube);
    assert_eq!(uploads.len(), 2);
    assert!(uploads.iter().all(|upload| upload.1 == reserved[0].1));
    cleanup(pool).await;
}

async fn review_with_reservations(pool: &PgPool, name: &str) -> i64 {
    let id = job_video(pool, name).await;
    approve(
        pool,
        "owner",
        id,
        &consented(pool, id, &["youtube", "tiktok"]).await,
    )
    .await
    .expect("reserve destinations");
    sqlx::query("DELETE FROM twitch_clips_upload_queue WHERE produced_video_id = $1")
        .bind(id)
        .execute(pool)
        .await
        .expect("no uploads yet");
    sqlx::query("UPDATE social_produced_videos SET status = 'review' WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .expect("back in review");
    id
}

async fn retarget(pool: &PgPool, platforms: &[&str]) {
    crate::production_jobs::autopilot_schedule::save_schedule(
        pool,
        "owner",
        &crate::production_jobs::autopilot_schedule::SchedulePatch {
            target_platforms: Some(platforms.iter().map(|value| (*value).to_owned()).collect()),
            ..Default::default()
        },
    )
    .await
    .expect("retarget");
}

#[tokio::test]
async fn target_change_rederives_destination_reservations() {
    let pool = pool().await;
    split_schedule(&pool, "same_clip").await;
    let id = review_with_reservations(&pool, "review").await;
    let (before, _) = video_slots(&pool, id).await;
    assert_eq!(before.len(), 2);
    retarget(&pool, &["youtube"]).await;
    let (reserved, _) = video_slots(&pool, id).await;
    assert_eq!(reserved, vec![before[1].clone()]);
    retarget(&pool, &["youtube", "tiktok"]).await;
    let (reserved, _) = video_slots(&pool, id).await;
    assert_eq!(reserved, before);
    cleanup(pool).await;
}

#[tokio::test]
async fn independent_clip_moves_to_a_remaining_destination() {
    let pool = pool().await;
    split_schedule(&pool, "independent_clips").await;
    let (id, _) = video(&pool, "solo").await;
    let job: i64 = sqlx::query_scalar(
        "INSERT INTO social_production_jobs (twitch_user_id, clip_id, queue_order, status, mode, started_at, finished_at)
        SELECT twitch_user_id, clip_id, 1, 'completed', 'static', NOW(), NOW() FROM social_produced_videos WHERE id = $1
        RETURNING id",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("completed job");
    let evening: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT ((date_trunc('day', clock_timestamp() AT TIME ZONE s.timezone) + INTERVAL '2 days 18 hours')
            AT TIME ZONE s.timezone) FROM social_media_autopilot_settings s WHERE s.twitch_user_id = 'owner'",
    )
    .fetch_one(&pool)
    .await
    .expect("tiktok slot");
    sqlx::query("UPDATE social_produced_videos SET job_id = $2, scheduled_at = $3 WHERE id = $1")
        .bind(id)
        .bind(job)
        .bind(evening)
        .execute(&pool)
        .await
        .expect("video slot");
    sqlx::query("INSERT INTO social_publication_reservations (twitch_user_id, platform, scheduled_at, job_id, produced_video_id)
        VALUES ('owner', 'tiktok', $2, $1, $3)")
        .bind(job)
        .bind(evening)
        .bind(id)
        .execute(&pool)
        .await
        .expect("tiktok reservation");
    retarget(&pool, &["youtube"]).await;
    let (reserved, _) = video_slots(&pool, id).await;
    assert_eq!(reserved.len(), 1);
    assert_eq!(reserved[0].0, "youtube");
    assert!(reserved[0].1 > chrono::Utc::now());
    cleanup(pool).await;
}

#[tokio::test]
async fn late_event_keeps_its_valid_future_destination_slot() {
    let pool = pool().await;
    split_schedule(&pool, "same_clip").await;
    let id = job_video(&pool, "late").await;
    approve(
        &pool,
        "owner",
        id,
        &consented(&pool, id, &["youtube", "tiktok"]).await,
    )
    .await
    .expect("approve");
    let (_, before) = video_slots(&pool, id).await;
    sqlx::query("UPDATE social_produced_videos SET scheduled_at = clock_timestamp() - INTERVAL '3 hours' WHERE id = $1")
        .bind(id).execute(&pool).await.expect("late video");
    sqlx::query("UPDATE twitch_clips_upload_queue SET scheduled_at = clock_timestamp() - INTERVAL '3 hours' WHERE produced_video_id = $1 AND platform = 'youtube'")
        .bind(id).execute(&pool).await.expect("late youtube");
    let publisher = ScriptedPublisher::with(vec![]);
    let report = PublishExecutor::new(pool.clone(), publisher.clone())
        .tick()
        .await
        .expect("tick");
    assert_eq!(report.rescheduled, 1);
    assert!(publisher.calls().is_empty());
    let (reserved, uploads) = video_slots(&pool, id).await;
    assert_eq!(uploads, before);
    assert_eq!(reserved, before);
    cleanup(pool).await;
}

enum TikTokStep {
    Fail(UploadError),
    CheckpointFails,
    Transfer(&'static str, Result<(), UploadError>),
}

struct TikTokFake {
    pool: PgPool,
    statuses: Mutex<HashMap<String, Value>>,
    polls: Mutex<Vec<String>>,
    steps: Mutex<Vec<TikTokStep>>,
    calls: Mutex<Vec<(String, TikTokPostOptions)>>,
    persisted_before_transfer: Mutex<Vec<bool>>,
}

impl TikTokFake {
    fn with(pool: &PgPool, steps: Vec<TikTokStep>) -> Arc<Self> {
        Arc::new(Self {
            pool: pool.clone(),
            statuses: Mutex::default(),
            polls: Mutex::default(),
            steps: Mutex::new(steps),
            calls: Mutex::default(),
            persisted_before_transfer: Mutex::default(),
        })
    }

    fn calls(&self) -> Vec<(String, TikTokPostOptions)> {
        self.calls.lock().expect("calls").clone()
    }

    fn report(&self, publish_id: &str, status: Value) {
        self.statuses
            .lock()
            .expect("statuses")
            .insert(publish_id.into(), status);
    }

    fn polls(&self) -> Vec<String> {
        self.polls.lock().expect("polls").clone()
    }

    fn persisted(&self) -> Vec<bool> {
        self.persisted_before_transfer
            .lock()
            .expect("persisted")
            .clone()
    }
}

#[async_trait]
impl DestinationPublisher for TikTokFake {
    async fn publish(
        &self,
        _owner: &str,
        _platform: &str,
        _request: &PublishRequest,
    ) -> Result<PublishOutcome, UploadError> {
        Err(UploadError::Api("unexpected generic upload".into()))
    }

    async fn publish_tiktok(
        &self,
        _owner: &str,
        request: &PublishRequest,
        options: &TikTokPostOptions,
        checkpoint: &dyn UploadCheckpoint,
    ) -> Result<String, UploadError> {
        self.calls
            .lock()
            .expect("calls")
            .push((request.file_path.clone(), options.clone()));
        let step = {
            let mut steps = self.steps.lock().expect("steps");
            if steps.is_empty() {
                return Err(UploadError::Api("unexpected TikTok upload".into()));
            }
            steps.remove(0)
        };
        match step {
            TikTokStep::Fail(error) => Err(error),
            TikTokStep::CheckpointFails => {
                checkpoint.record_publish_id(" ").await?;
                Ok("never".into())
            }
            TikTokStep::Transfer(publish_id, transfer) => {
                checkpoint.record_publish_id(publish_id).await?;
                let persisted: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM twitch_clips_upload_queue WHERE platform = 'tiktok'
                     AND tiktok_publish_id = $1 AND status = $2)",
                )
                .bind(publish_id)
                .bind(queue_status::EFFECT_STARTED)
                .fetch_one(&self.pool)
                .await
                .expect("checkpoint lookup");
                self.persisted_before_transfer
                    .lock()
                    .expect("persisted")
                    .push(persisted);
                transfer.map(|()| publish_id.to_string())
            }
        }
    }

    async fn tiktok_status(
        &self,
        _owner: &str,
        _options: &TikTokPostOptions,
        publish_id: &str,
    ) -> Result<Value, UploadError> {
        self.polls.lock().expect("polls").push(publish_id.into());
        Ok(self
            .statuses
            .lock()
            .expect("statuses")
            .get(publish_id)
            .cloned()
            .unwrap_or_else(|| json!({})))
    }
}

async fn tiktok_ready(pool: &PgPool, name: &str) -> i64 {
    let (id, _) = video(pool, name).await;
    approve(pool, "owner", id, &consented(pool, id, &["tiktok"]).await)
        .await
        .expect("tiktok consent");
    make_due(pool, id).await;
    id
}

async fn tiktok_row(pool: &PgPool, id: i64) -> (Option<String>, Option<Value>, bool, bool) {
    sqlx::query_as(
        "SELECT q.tiktok_publish_id, q.tiktok_post_options, q.effect_started_at IS NOT NULL,
            COALESCE(c.uploaded_tiktok, FALSE)
         FROM twitch_clips_upload_queue q JOIN twitch_clips_social_media c ON c.id = q.clip_id
         WHERE q.produced_video_id = $1 AND q.platform = 'tiktok'",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("tiktok row")
}

async fn unify(pool: &PgPool) {
    sqlx::query(
        "INSERT INTO social_pipeline_cutover (twitch_user_id, state, migration_version)
         VALUES ('owner', 'unified', 1) ON CONFLICT DO NOTHING",
    )
    .execute(pool)
    .await
    .expect("cutover");
}

async fn dispatching(pool: &PgPool, fake: &Arc<TikTokFake>) -> PublishExecutor {
    unify(pool).await;
    PublishExecutor::new(pool.clone(), fake.clone()).with_tiktok_dispatch(true)
}

#[tokio::test]
async fn disabled_tiktok_dispatch_holds_with_the_specific_binding_reason() {
    let pool = pool().await;
    configure(&pool, &["tiktok"], &[("tiktok", "tt-1")]).await;
    let fake = TikTokFake::with(&pool, vec![]);
    let executor = PublishExecutor::new(pool.clone(), fake.clone());
    let bound = tiktok_ready(&pool, "bound").await;
    executor.tick().await.expect("bound tick");
    assert_eq!(
        queue_state(&pool, bound, "tiktok").await,
        (
            queue_status::HELD.into(),
            Some("tiktok_effect_boundary_pending".into())
        )
    );
    let revoked = tiktok_ready(&pool, "revoked").await;
    sqlx::query("DELETE FROM social_media_settings WHERE key LIKE 'tiktok_music_consent:%'")
        .execute(&pool)
        .await
        .expect("revoke music consent");
    executor.tick().await.expect("revoked tick");
    assert_eq!(
        queue_state(&pool, revoked, "tiktok").await,
        (
            queue_status::HELD.into(),
            Some("tiktok_music_consent_missing".into())
        )
    );
    dispatching(&pool, &fake)
        .await
        .tick()
        .await
        .expect("dispatch tick");
    assert!(fake.calls().is_empty());
    cleanup(pool).await;
}

#[tokio::test]
async fn tiktok_dispatch_persists_publish_id_before_transfer_and_never_repeats() {
    let pool = pool().await;
    configure(&pool, &["tiktok"], &[("tiktok", "tt-1")]).await;
    let id = tiktok_ready(&pool, "dispatch").await;
    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET title = 'queue row title' WHERE produced_video_id = $1",
    )
    .bind(id)
    .execute(&pool)
    .await
    .expect("queue title");
    let fake = TikTokFake::with(&pool, vec![TikTokStep::Transfer("pub-1", Ok(()))]);
    let executor = dispatching(&pool, &fake).await;
    executor.tick().await.expect("dispatch tick");
    let bound: TikTokPostOptions =
        serde_json::from_value(bound_options(&pool, id).await.expect("binding"))
            .expect("bound options");
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, bound);
    assert_ne!(calls[0].1.caption, "queue row title");
    assert_eq!(fake.persisted(), vec![true]);
    assert_eq!(
        queue_state(&pool, id, "tiktok").await,
        (
            queue_status::PROCESSING.into(),
            Some("tiktok_processing".into())
        )
    );
    let (publish_id, options, _, uploaded) = tiktok_row(&pool, id).await;
    assert_eq!(publish_id.as_deref(), Some("pub-1"));
    assert_eq!(
        options,
        Some(serde_json::to_value(&bound).expect("options json"))
    );
    assert!(!uploaded);
    executor.tick().await.expect("second tick");
    executor.tick().await.expect("third tick");
    assert_eq!(fake.calls().len(), 1);
    cleanup(pool).await;
}

#[tokio::test]
async fn failures_around_init_checkpoint_and_transfer_never_start_a_second_operation() {
    let pool = pool().await;
    configure(&pool, &["tiktok"], &[("tiktok", "tt-1")]).await;
    let init = tiktok_ready(&pool, "init").await;
    let checkpoint = tiktok_ready(&pool, "checkpoint").await;
    let transfer = tiktok_ready(&pool, "transfer").await;
    let fake = TikTokFake::with(
        &pool,
        vec![
            TikTokStep::Fail(UploadError::Request("timeout".into())),
            TikTokStep::CheckpointFails,
            TikTokStep::Transfer("pub-2", Err(UploadError::Api("chunk failed".into()))),
        ],
    );
    let executor = dispatching(&pool, &fake).await;
    executor.tick().await.expect("failure tick");
    assert_eq!(fake.calls().len(), 3);
    for id in [init, checkpoint] {
        assert_eq!(
            queue_state(&pool, id, "tiktok").await,
            (
                queue_status::RETRY.into(),
                Some("tiktok_init_unconfirmed".into())
            )
        );
        let (publish_id, _, started, _) = tiktok_row(&pool, id).await;
        assert!(publish_id.is_none());
        assert!(!started);
    }
    assert_eq!(
        queue_state(&pool, transfer, "tiktok").await,
        (
            queue_status::UNCERTAIN.into(),
            Some("tiktok_transfer_uncertain".into())
        )
    );
    assert_eq!(
        tiktok_row(&pool, transfer).await.0.as_deref(),
        Some("pub-2")
    );
    assert!(matches!(
        retry(&pool, "owner", transfer, "tiktok").await,
        Err(PublishError::RetryBlocked)
    ));
    executor.tick().await.expect("second tick");
    assert_eq!(fake.calls().len(), 3);
    assert!(fake.polls().is_empty());
    cleanup(pool).await;
}

#[tokio::test]
async fn claim_checkpoint_is_fenced_by_generation_and_written_once() {
    let pool = pool().await;
    configure(&pool, &["tiktok"], &[("tiktok", "tt-1")]).await;
    let id = tiktok_ready(&pool, "fenced").await;
    PublishExecutor::new(pool.clone(), ScriptedPublisher::with(vec![]))
        .tick()
        .await
        .expect("held tick");
    let queue_id: i64 = sqlx::query_scalar(
        "UPDATE twitch_clips_upload_queue SET status = $2, claim_generation = 3
         WHERE produced_video_id = $1 AND platform = 'tiktok' RETURNING id::bigint",
    )
    .bind(id)
    .bind(queue_status::EFFECT_STARTED)
    .fetch_one(&pool)
    .await
    .expect("effect started");
    let at = |generation| crate::tiktok_recovery::ClaimCheckpoint {
        pool: &pool,
        queue_id,
        generation,
    };
    assert!(at(2).record_publish_id("stale").await.is_err());
    at(3).record_publish_id("first").await.expect("checkpoint");
    assert!(at(3).record_publish_id("second").await.is_err());
    assert_eq!(tiktok_row(&pool, id).await.0.as_deref(), Some("first"));
    cleanup(pool).await;
}

#[tokio::test]
async fn rejected_tiktok_choice_is_held_editable_and_dispatched_once_after_rebinding() {
    let pool = pool().await;
    configure(&pool, &["tiktok"], &[("tiktok", "tt-1")]).await;
    let id = tiktok_ready(&pool, "rejected").await;
    let fake = TikTokFake::with(
        &pool,
        vec![
            TikTokStep::Fail(UploadError::not_started(UploadError::Validation(
                "privacy level not allowed".into(),
            ))),
            TikTokStep::Transfer("pub-3", Ok(())),
        ],
    );
    let executor = dispatching(&pool, &fake).await;
    executor.tick().await.expect("rejected tick");
    assert_eq!(
        queue_state(&pool, id, "tiktok").await,
        (
            queue_status::HELD.into(),
            Some("tiktok_options_invalid".into())
        )
    );
    let (publish_id, _, started, _) = tiktok_row(&pool, id).await;
    assert!(publish_id.is_none());
    assert!(!started);
    let mut choice = tiktok_choice(&pool, id).await;
    choice["privacy_level"] = json!("FOLLOWER_OF_CREATOR");
    approve(&pool, "owner", id, &with_tiktok(choice))
        .await
        .expect("rebind");
    sqlx::query("UPDATE twitch_clips_upload_queue SET next_retry_at = clock_timestamp() - INTERVAL '1 minute' WHERE produced_video_id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("due retry");
    executor.tick().await.expect("rebound tick");
    let calls = fake.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].1.privacy_level, "FOLLOWER_OF_CREATOR");
    assert_eq!(
        queue_state(&pool, id, "tiktok").await,
        (
            queue_status::PROCESSING.into(),
            Some("tiktok_processing".into())
        )
    );
    executor.tick().await.expect("settled tick");
    assert_eq!(fake.calls().len(), 2);
    cleanup(pool).await;
}

#[tokio::test]
async fn retry_row_with_a_known_publish_id_is_quarantined_without_upload() {
    let pool = pool().await;
    configure(&pool, &["tiktok"], &[("tiktok", "tt-1")]).await;
    let id = tiktok_ready(&pool, "ghost").await;
    let fake = TikTokFake::with(&pool, vec![TikTokStep::Transfer("pub-4", Ok(()))]);
    PublishExecutor::new(pool.clone(), fake.clone())
        .tick()
        .await
        .expect("held tick");
    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET status = $2, tiktok_publish_id = 'ghost',
         next_retry_at = clock_timestamp() - INTERVAL '1 minute' WHERE produced_video_id = $1",
    )
    .bind(id)
    .bind(queue_status::RETRY)
    .execute(&pool)
    .await
    .expect("retry with publish id");
    dispatching(&pool, &fake)
        .await
        .tick()
        .await
        .expect("quarantine tick");
    assert!(fake.calls().is_empty());
    assert_eq!(
        queue_state(&pool, id, "tiktok").await,
        (
            queue_status::UNCERTAIN.into(),
            Some("tiktok_transfer_uncertain".into())
        )
    );
    cleanup(pool).await;
}

async fn age_tiktok(pool: &PgPool, id: i64, attempt: &str, started: Option<&str>) {
    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET last_attempt_at = clock_timestamp() - $2::interval,
         effect_started_at = COALESCE(clock_timestamp() - $3::interval, effect_started_at)
         WHERE produced_video_id = $1 AND platform = 'tiktok'",
    )
    .bind(id)
    .bind(attempt)
    .bind(started)
    .execute(pool)
    .await
    .expect("aged tiktok row");
}

async fn unclear_alerts(pool: &PgPool) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM social_autopilot_notifications WHERE content LIKE '%unklar%'",
    )
    .fetch_one(pool)
    .await
    .expect("alerts")
}

async fn external_id(pool: &PgPool, id: i64) -> Option<String> {
    sqlx::query_scalar(
        "SELECT external_id FROM twitch_clips_upload_queue WHERE produced_video_id = $1 AND platform = 'tiktok'",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("external id")
}

#[tokio::test]
async fn reconciliation_settles_processing_rows_without_unclear_alerts() {
    let pool = pool().await;
    configure(&pool, &["tiktok"], &[("tiktok", "tt-1")]).await;
    let posted = tiktok_ready(&pool, "posted").await;
    let inbox = tiktok_ready(&pool, "inbox").await;
    let rejected = tiktok_ready(&pool, "rejected").await;
    let slow = tiktok_ready(&pool, "slow").await;
    let fake = TikTokFake::with(
        &pool,
        vec![
            TikTokStep::Transfer("pub-a", Ok(())),
            TikTokStep::Transfer("pub-b", Ok(())),
            TikTokStep::Transfer("pub-c", Ok(())),
            TikTokStep::Transfer("pub-d", Ok(())),
        ],
    );
    let executor = dispatching(&pool, &fake).await;
    executor.tick().await.expect("dispatch tick");
    assert!(fake.polls().is_empty());
    for id in [posted, inbox, rejected, slow] {
        assert_eq!(
            queue_state(&pool, id, "tiktok").await.0,
            queue_status::PROCESSING
        );
        age_tiktok(&pool, id, "2 minutes", None).await;
    }
    fake.report(
        "pub-a",
        json!({"status": "PUBLISH_COMPLETE", "publicaly_available_post_id": ["post-a"]}),
    );
    fake.report("pub-b", json!({"status": "SEND_TO_USER_INBOX"}));
    fake.report(
        "pub-c",
        json!({"status": "FAILED", "fail_reason": "spam_risk"}),
    );
    fake.report("pub-d", json!({"status": "PROCESSING_UPLOAD"}));
    let report = executor.tick().await.expect("reconcile tick");
    assert_eq!(report.reconciled, 3);
    assert_eq!(fake.polls().len(), 4);
    assert_eq!(
        queue_state(&pool, posted, "tiktok").await,
        (queue_status::PUBLISHED.into(), None)
    );
    assert_eq!(external_id(&pool, posted).await.as_deref(), Some("post-a"));
    assert!(tiktok_row(&pool, posted).await.3);
    assert_eq!(
        queue_state(&pool, inbox, "tiktok").await,
        (
            queue_status::FAILED.into(),
            Some("tiktok_inbox_only".into())
        )
    );
    assert!(!tiktok_row(&pool, inbox).await.3);
    assert_eq!(
        queue_state(&pool, rejected, "tiktok").await,
        (queue_status::FAILED.into(), Some("tiktok_rejected".into()))
    );
    assert_eq!(
        queue_state(&pool, slow, "tiktok").await,
        (
            queue_status::PROCESSING.into(),
            Some("tiktok_processing".into())
        )
    );
    assert_eq!(unclear_alerts(&pool).await, 0);
    assert_eq!(fake.calls().len(), 4);
    age_tiktok(&pool, slow, "2 minutes", Some("25 hours")).await;
    executor.tick().await.expect("escalation tick");
    assert_eq!(
        queue_state(&pool, slow, "tiktok").await,
        (
            queue_status::UNCERTAIN.into(),
            Some("tiktok_status_unconfirmed".into())
        )
    );
    assert_eq!(unclear_alerts(&pool).await, 1);
    assert_eq!(fake.calls().len(), 4);
    cleanup(pool).await;
}

#[tokio::test]
async fn restart_recovers_started_rows_by_status_or_safe_retry() {
    let pool = pool().await;
    configure(&pool, &["tiktok"], &[("tiktok", "tt-1")]).await;
    let transferred = tiktok_ready(&pool, "transferred").await;
    let unsent = tiktok_ready(&pool, "unsent").await;
    let fake = TikTokFake::with(
        &pool,
        vec![
            TikTokStep::Transfer("pub-r", Ok(())),
            TikTokStep::Fail(UploadError::Request("timeout".into())),
        ],
    );
    let executor = dispatching(&pool, &fake).await;
    executor.tick().await.expect("dispatch tick");
    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET status = $2, next_retry_at = NULL,
         outcome_reason = NULL WHERE produced_video_id = ANY($1) AND platform = 'tiktok'",
    )
    .bind(vec![transferred, unsent])
    .bind(queue_status::EFFECT_STARTED)
    .execute(&pool)
    .await
    .expect("crashed rows");
    for id in [transferred, unsent] {
        age_tiktok(&pool, id, "3 hours", Some("3 hours")).await;
    }
    fake.report(
        "pub-r",
        json!({"status": "PUBLISH_COMPLETE", "publicaly_available_post_id": ["post-r"]}),
    );
    executor.tick().await.expect("restart tick");
    assert_eq!(
        queue_state(&pool, transferred, "tiktok").await,
        (queue_status::PUBLISHED.into(), None)
    );
    assert_eq!(
        external_id(&pool, transferred).await.as_deref(),
        Some("post-r")
    );
    assert_eq!(
        queue_state(&pool, unsent, "tiktok").await,
        (
            queue_status::RETRY.into(),
            Some("tiktok_init_unconfirmed".into())
        )
    );
    assert_eq!(fake.calls().len(), 2);
    assert_eq!(fake.polls(), vec!["pub-r".to_string()]);
    cleanup(pool).await;
}

#[tokio::test]
async fn owners_without_unified_cutover_are_held_until_switched() {
    let pool = pool().await;
    configure(&pool, &["tiktok"], &[("tiktok", "tt-1")]).await;
    let id = tiktok_ready(&pool, "legacy").await;
    let fake = TikTokFake::with(&pool, vec![TikTokStep::Transfer("pub-u", Ok(()))]);
    let executor = PublishExecutor::new(pool.clone(), fake.clone()).with_tiktok_dispatch(true);
    executor.tick().await.expect("legacy tick");
    assert!(fake.calls().is_empty());
    assert_eq!(
        queue_state(&pool, id, "tiktok").await,
        (
            queue_status::HELD.into(),
            Some("tiktok_effect_boundary_pending".into())
        )
    );
    approve(&pool, "owner", id, &consented(&pool, id, &["tiktok"]).await)
        .await
        .expect("reapprove");
    assert_eq!(
        queue_state(&pool, id, "tiktok").await.0,
        queue_status::RETRY
    );
    unify(&pool).await;
    sqlx::query("UPDATE twitch_clips_upload_queue SET next_retry_at = clock_timestamp() - INTERVAL '1 minute' WHERE produced_video_id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("due retry");
    executor.tick().await.expect("unified tick");
    assert_eq!(fake.calls().len(), 1);
    assert_eq!(
        queue_state(&pool, id, "tiktok").await.0,
        queue_status::PROCESSING
    );
    cleanup(pool).await;
}

#[tokio::test]
async fn legacy_and_autopilot_rows_never_transfer_the_same_clip_twice() {
    let pool = pool().await;
    configure(&pool, &["tiktok"], &[("tiktok", "tt-1")]).await;
    let id = tiktok_ready(&pool, "fenced").await;
    let clip: i64 = sqlx::query_scalar(
        "SELECT clip_id::bigint FROM twitch_clips_upload_queue WHERE produced_video_id = $1 AND platform = 'tiktok'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("clip");
    let legacy: i64 = sqlx::query_scalar(
        "INSERT INTO twitch_clips_upload_queue (clip_id, platform, status, title)
         VALUES ($1, 'tiktok', 'inbox_pending', 'legacy') RETURNING id::bigint",
    )
    .bind(clip)
    .fetch_one(&pool)
    .await
    .expect("legacy row");
    let fake = TikTokFake::with(&pool, vec![TikTokStep::Transfer("pub-f", Ok(()))]);
    let executor = dispatching(&pool, &fake).await;
    executor.tick().await.expect("fenced tick");
    assert!(fake.calls().is_empty());
    assert_eq!(
        queue_state(&pool, id, "tiktok").await,
        (
            queue_status::HELD.into(),
            Some("tiktok_transfer_active".into())
        )
    );
    sqlx::query("UPDATE twitch_clips_upload_queue SET status = 'failed' WHERE id = $1")
        .bind(legacy)
        .execute(&pool)
        .await
        .expect("legacy settled");
    approve(&pool, "owner", id, &consented(&pool, id, &["tiktok"]).await)
        .await
        .expect("reapprove");
    sqlx::query("UPDATE twitch_clips_upload_queue SET next_retry_at = clock_timestamp() - INTERVAL '1 minute' WHERE produced_video_id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("due retry");
    executor.tick().await.expect("autopilot tick");
    assert_eq!(fake.calls().len(), 1);
    sqlx::query("UPDATE twitch_clips_upload_queue SET status = 'pending' WHERE id = $1")
        .bind(legacy)
        .execute(&pool)
        .await
        .expect("legacy pending");
    assert!(!crate::tiktok_recovery::reserve(&pool, legacy)
        .await
        .expect("legacy reserve"));
    cleanup(pool).await;
}

#[tokio::test]
async fn unusable_files_are_held_with_their_own_reason() {
    let pool = pool().await;
    configure(&pool, &["tiktok"], &[("tiktok", "tt-1")]).await;
    let id = tiktok_ready(&pool, "file").await;
    let fake = TikTokFake::with(
        &pool,
        vec![TikTokStep::Fail(UploadError::not_started(UploadError::Io(
            std::io::Error::other("unreadable"),
        )))],
    );
    dispatching(&pool, &fake)
        .await
        .tick()
        .await
        .expect("file tick");
    assert_eq!(
        queue_state(&pool, id, "tiktok").await,
        (
            queue_status::HELD.into(),
            Some("tiktok_file_invalid".into())
        )
    );
    let (publish_id, _, started, _) = tiktok_row(&pool, id).await;
    assert!(publish_id.is_none());
    assert!(!started);
    cleanup(pool).await;
}

#[tokio::test]
async fn snapshot_consent_follows_music_consent() {
    let pool = pool().await;
    configure(&pool, &["tiktok"], &[("tiktok", "tt-1")]).await;
    let id = tiktok_ready(&pool, "music").await;
    assert_eq!(tiktok_state(&pool, id).await, "bound");
    sqlx::query("DELETE FROM social_media_settings WHERE key LIKE 'tiktok_music_consent:%'")
        .execute(&pool)
        .await
        .expect("revoke music consent");
    assert_eq!(tiktok_state(&pool, id).await, "stale");
    cleanup(pool).await;
}

async fn veto_video(pool: &PgPool, name: &str, policy: &str, deadline_minutes: i32) -> i64 {
    let id = job_video(pool, name).await;
    sqlx::query(
        "UPDATE social_produced_videos SET needs_review = FALSE,
         policy_snapshot = jsonb_build_object('review_policy', $2::text),
         scheduled_at = clock_timestamp() + make_interval(mins => $3),
         metadata = metadata || jsonb_build_object('veto_deadline',
             to_jsonb(clock_timestamp() + make_interval(mins => $3)))
         WHERE id = $1",
    )
    .bind(id)
    .bind(policy)
    .bind(deadline_minutes)
    .execute(pool)
    .await
    .expect("veto video");
    id
}

async fn video_status(pool: &PgPool, id: i64) -> String {
    sqlx::query_scalar("SELECT status FROM social_produced_videos WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("video status")
}

#[tokio::test]
async fn veto_publishes_only_after_the_deadline_and_never_a_vetoed_video() {
    let pool = pool().await;
    configure(&pool, &["youtube"], &[("youtube", "yt-1")]).await;
    sqlx::query("UPDATE social_media_autopilot_settings SET review_policy = 'veto_until_deadline' WHERE twitch_user_id = 'owner'")
        .execute(&pool)
        .await
        .expect("veto policy");
    let waiting = veto_video(&pool, "waiting", "veto_until_deadline", 30).await;
    let due = veto_video(&pool, "due", "veto_until_deadline", -1).await;
    let vetoed = veto_video(&pool, "vetoed", "veto_until_deadline", -1).await;
    sqlx::query("UPDATE social_produced_videos SET status = 'rejected' WHERE id = $1")
        .bind(vetoed)
        .execute(&pool)
        .await
        .expect("vetoed");
    let declined = veto_video(&pool, "declined", "veto_until_deadline", -1).await;
    sqlx::query("INSERT INTO social_media_clip_approval (produced_video_id, platform, state, approved_platforms, approver_user_id, decided_at) VALUES ($1, 'youtube', 'rejected', '[]', 'owner', NOW())")
        .bind(declined)
        .execute(&pool)
        .await
        .expect("declined destination");
    let undated = veto_video(&pool, "undated", "veto_until_deadline", -1).await;
    sqlx::query(
        "UPDATE social_produced_videos SET metadata = metadata - 'veto_deadline' WHERE id = $1",
    )
    .bind(undated)
    .execute(&pool)
    .await
    .expect("no deadline");
    let malformed = veto_video(&pool, "malformed", "veto_until_deadline", -1).await;
    sqlx::query(
        "UPDATE social_produced_videos SET metadata = metadata || '{\"veto_deadline\": \"soon\"}' WHERE id = $1",
    )
    .bind(malformed)
    .execute(&pool)
    .await
    .expect("malformed deadline");
    let frozen = veto_video(&pool, "frozen", "required", -1).await;
    let view = snapshot(&pool, "owner").await.expect("snapshot");
    assert_eq!(view.settings.review_mode, ReviewMode::VetoUntilDeadline);
    assert_eq!(
        serde_json::to_value(&view.settings).expect("settings")["review_mode"],
        "veto_until_deadline"
    );
    let publisher = ScriptedPublisher::with(vec![published("yt-due")]);
    let executor = PublishExecutor::new(pool.clone(), publisher.clone());
    executor.tick().await.expect("tick");
    executor.tick().await.expect("second tick");
    let calls: Vec<String> = publisher
        .calls()
        .into_iter()
        .map(|(platform, _)| platform)
        .collect();
    assert_eq!(calls, vec!["youtube".to_string()]);
    assert_eq!(
        destinations(&pool, due).await,
        vec![("youtube".into(), queue_status::PUBLISHED.into(), None)]
    );
    assert_eq!(video_status(&pool, due).await, "published");
    assert_eq!(video_status(&pool, waiting).await, "review");
    assert_eq!(approval_count(&pool, waiting).await, 0);
    assert_eq!(video_status(&pool, vetoed).await, "rejected");
    for id in [vetoed, declined, undated, malformed, frozen] {
        let automatic: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM social_media_clip_approval WHERE produced_video_id = $1 AND approver_user_id = 'autopilot'",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("automatic approvals");
        assert_eq!(automatic, 0);
        assert!(destinations(&pool, id)
            .await
            .iter()
            .all(|(_, status, _)| status != queue_status::PUBLISHED));
    }
    cleanup(pool).await;
}

#[tokio::test]
async fn veto_approvals_lapse_when_the_channel_returns_to_review() {
    let pool = pool().await;
    configure(&pool, &["youtube"], &[("youtube", "yt-1")]).await;
    sqlx::query("UPDATE social_media_autopilot_settings SET review_policy = 'veto_until_deadline' WHERE twitch_user_id = 'owner'")
        .execute(&pool)
        .await
        .expect("veto policy");
    let id = veto_video(&pool, "lapsed", "veto_until_deadline", 30).await;
    sqlx::query("UPDATE social_produced_videos SET metadata = jsonb_set(metadata, '{veto_deadline}', to_jsonb(clock_timestamp() - INTERVAL '1 minute')) WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("deadline passed");
    let publisher = ScriptedPublisher::with(vec![]);
    let executor = PublishExecutor::new(pool.clone(), publisher.clone());
    executor.tick().await.expect("promotion tick");
    assert_eq!(video_status(&pool, id).await, "approved");
    sqlx::query("UPDATE social_media_autopilot_settings SET review_policy = 'required' WHERE twitch_user_id = 'owner'")
        .execute(&pool)
        .await
        .expect("review again");
    assert_eq!(
        snapshot(&pool, "owner")
            .await
            .expect("snapshot")
            .settings
            .review_mode,
        ReviewMode::Review
    );
    make_due(&pool, id).await;
    executor.tick().await.expect("due tick");
    assert!(publisher.calls().is_empty());
    assert_eq!(video_status(&pool, id).await, "review");
    cleanup(pool).await;
}

#[tokio::test]
async fn unified_review_policy_off_publishes_despite_legacy_review_mode() {
    let pool = pool().await;
    configure(&pool, &["youtube"], &[("youtube", "yt-1")]).await;
    sqlx::query("UPDATE social_media_autopilot_settings SET pipeline_mode = 'ai_edit', review_policy = 'off', publish_mode = 'review' WHERE twitch_user_id = 'owner'")
        .execute(&pool)
        .await
        .expect("unified off policy");
    let automatic = veto_video(&pool, "off", "off", 30).await;
    let held = veto_video(&pool, "held", "off", 30).await;
    sqlx::query("UPDATE social_produced_videos SET needs_review = TRUE WHERE id = $1")
        .bind(held)
        .execute(&pool)
        .await
        .expect("quality hold");
    let revised = veto_video(&pool, "revised", "off", 30).await;
    sqlx::query("UPDATE social_produced_videos SET metadata = metadata || '{\"manual_ai_job_id\": 1}' WHERE id = $1")
        .bind(revised)
        .execute(&pool)
        .await
        .expect("manual revision");
    assert_eq!(
        snapshot(&pool, "owner")
            .await
            .expect("snapshot")
            .settings
            .review_mode,
        ReviewMode::FullAuto
    );
    let publisher = ScriptedPublisher::with(vec![published("yt-off")]);
    let executor = PublishExecutor::new(pool.clone(), publisher.clone());
    executor.tick().await.expect("promotion tick");
    assert_ne!(video_status(&pool, automatic).await, "review");
    make_due(&pool, automatic).await;
    executor.tick().await.expect("due tick");
    assert_eq!(video_status(&pool, automatic).await, "published");
    assert_eq!(publisher.calls().len(), 1);
    for id in [held, revised] {
        assert_eq!(video_status(&pool, id).await, "review");
        assert_eq!(approval_count(&pool, id).await, 0);
    }
    cleanup(pool).await;
}

#[tokio::test]
async fn impossible_veto_deadline_holds_only_its_own_row() {
    let pool = pool().await;
    configure(&pool, &["youtube"], &[("youtube", "yt-1")]).await;
    sqlx::query("UPDATE social_media_autopilot_settings SET review_policy = 'veto_until_deadline' WHERE twitch_user_id = 'owner'")
        .execute(&pool)
        .await
        .expect("veto policy");
    let due = veto_video(&pool, "due", "veto_until_deadline", -1).await;
    let broken = veto_video(&pool, "broken", "veto_until_deadline", -1).await;
    sqlx::query("UPDATE social_produced_videos SET metadata = metadata || '{\"veto_deadline\": \"2026-13-01T00:00:00Z\"}' WHERE id = $1")
        .bind(broken)
        .execute(&pool)
        .await
        .expect("impossible deadline");
    let publisher = ScriptedPublisher::with(vec![published("yt-due")]);
    let executor = PublishExecutor::new(pool.clone(), publisher.clone());
    executor.tick().await.expect("tick survives the broken row");
    executor.tick().await.expect("second tick");
    assert_eq!(video_status(&pool, due).await, "published");
    assert_eq!(approval_count(&pool, broken).await, 0);
    assert_eq!(publisher.calls().len(), 1);
    cleanup(pool).await;
}

#[tokio::test]
async fn veto_deadline_passing_during_the_tick_publishes_in_that_tick() {
    let pool = pool().await;
    configure(&pool, &["youtube"], &[("youtube", "yt-1")]).await;
    sqlx::query("UPDATE social_media_autopilot_settings SET review_policy = 'veto_until_deadline' WHERE twitch_user_id = 'owner'")
        .execute(&pool)
        .await
        .expect("veto policy");
    let id = veto_video(&pool, "late", "veto_until_deadline", -1).await;
    sqlx::query("UPDATE social_produced_videos SET autopilot_deferred_stage = 'promote', autopilot_deferred_until = clock_timestamp() + INTERVAL '1 hour' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("missed by the promotion stage");
    let slot: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT scheduled_at FROM social_produced_videos WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("slot");
    let publisher = ScriptedPublisher::with(vec![published("yt-late")]);
    let executor = PublishExecutor::new(pool.clone(), publisher.clone());
    let report = executor.tick().await.expect("tick");
    assert_eq!(report.promoted, 1);
    assert_eq!(report.released, 0);
    assert_eq!(video_status(&pool, id).await, "published");
    let event: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT scheduled_at FROM social_produced_videos WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("event slot");
    assert_eq!(event, slot);
    cleanup(pool).await;
}

#[tokio::test]
async fn due_veto_slot_never_auto_approves_a_manual_revision() {
    let pool = pool().await;
    configure(&pool, &["youtube"], &[("youtube", "yt-1")]).await;
    sqlx::query("UPDATE social_media_autopilot_settings SET review_policy = 'veto_until_deadline' WHERE twitch_user_id = 'owner'")
        .execute(&pool)
        .await
        .expect("veto policy");
    let id = veto_video(&pool, "revising", "veto_until_deadline", -1).await;
    sqlx::query("UPDATE social_produced_videos SET metadata = metadata || '{\"manual_ai_job_id\": 7}' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .expect("manual revision");
    let publisher = ScriptedPublisher::with(vec![published("must-not-publish")]);
    let executor = PublishExecutor::new(pool.clone(), publisher.clone());
    let report = executor.tick().await.expect("tick");
    assert_eq!(report.promoted, 0);
    assert_eq!(approval_count(&pool, id).await, 0);
    assert_eq!(video_status(&pool, id).await, "review");
    assert!(publisher.calls().is_empty());
    cleanup(pool).await;
}

#[tokio::test]
async fn unconfirmed_publish_becomes_uncertain_without_replay() {
    let pool = pool().await;
    configure(&pool, &["youtube"], &[("youtube", "yt-1")]).await;
    let (id, _) = video(&pool, "unconfirmed").await;
    approve(&pool, "owner", id, &decisions(&["youtube"]))
        .await
        .expect("approve");
    make_due(&pool, id).await;
    let publisher = ScriptedPublisher::with(vec![
        Err(UploadError::PublishUnconfirmed(
            "No media ID in response".into(),
        )),
        published("must-not-replay"),
    ]);
    let executor = PublishExecutor::new(pool.clone(), publisher.clone());
    executor.tick().await.expect("first tick");
    executor.tick().await.expect("second tick");
    assert_eq!(
        destinations(&pool, id).await,
        vec![(
            "youtube".into(),
            queue_status::UNCERTAIN.into(),
            Some("publish_unconfirmed".into())
        )]
    );
    assert_eq!(publisher.calls().len(), 1);
    cleanup(pool).await;
}
