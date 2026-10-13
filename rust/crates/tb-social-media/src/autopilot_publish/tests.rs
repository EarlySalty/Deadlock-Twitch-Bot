use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::json;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;

use super::*;
use crate::autopilot_settings::{AutopilotSettings, ScheduleSlot};
use crate::produced_video::{NewProducedVideo, RenderedVideo};
use crate::production_jobs::ProductionMode;
use crate::uploaders::UploadError;

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
        CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT NOT NULL, twitch_login TEXT NOT NULL, discord_user_id TEXT, updated_at TEXT);
        CREATE TABLE social_media_platform_auth (id SERIAL PRIMARY KEY, platform TEXT NOT NULL, twitch_user_id TEXT,
            platform_user_id TEXT, enabled INTEGER DEFAULT 1, authorized_at TEXT DEFAULT CURRENT_TIMESTAMP);
        CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, twitch_user_id TEXT, clip_id TEXT,
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
            completed_at TIMESTAMPTZ, youtube_visibility TEXT, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW())")
        .execute(&pool).await.expect("legacy prerequisite schema");
    for migration in [
        include_str!("../../../../migrations/20261011100000_social_autopilot_contracts.sql"),
        include_str!("../../../../migrations/20261011101000_social_clip_pool.sql"),
        include_str!("../../../../migrations/20261011102000_social_production_slot_guards.sql"),
        include_str!("../../../../migrations/20261011105000_social_autopilot_publishing.sql"),
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
    approve(&pool, "owner", id, &decisions(&["youtube", "tiktok"]))
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
