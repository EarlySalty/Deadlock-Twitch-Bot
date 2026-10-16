#[path = "../../../test-support/postgres.rs"]
mod postgres;

use std::collections::BTreeMap;

use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tb_social_media::ai_editor::metadata::{self, MetadataError, PlatformCopy};
use tb_social_media::ai_editor::AiEditorError;
use tb_social_media::autopilot_settings::{self, PolicyScope, TextDefaults};
use tb_social_media::clip_templates;

const OWNER: &str = "100";

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
    sqlx::query("INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('alpha', $1)")
        .bind(OWNER)
        .execute(&pool)
        .await
        .expect("Streamer");
    (db, pool)
}

async fn clip(pool: &PgPool, key: &str, pool_managed: bool) -> i64 {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO twitch_clips_social_media (clip_id, clip_url, clip_title, streamer_login, twitch_user_id, game_name)
         VALUES ($1, 'https://clips.test/' || $1, 'Insane 1v3', 'alpha', $2, 'Deadlock') RETURNING id",
    )
    .bind(key)
    .bind(OWNER)
    .fetch_one(pool)
    .await
    .expect("Clip");
    if pool_managed {
        sqlx::query("UPDATE twitch_clips_social_media SET pool_managed_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await
            .expect("Pool-Verwaltung");
    }
    id
}

fn copy(title: &str, description: &str, hashtags: &[&str]) -> PlatformCopy {
    PlatformCopy {
        title: title.into(),
        description: description.into(),
        hashtags: hashtags.iter().map(|tag| (*tag).to_string()).collect(),
    }
}

fn all(copy: PlatformCopy) -> BTreeMap<String, PlatformCopy> {
    ["youtube", "tiktok", "instagram"]
        .into_iter()
        .map(|platform| (platform.to_string(), copy.clone()))
        .collect()
}

async fn version(pool: &PgPool, key: &str, mode: &str, metadata: Value) -> i64 {
    let clip = clip(pool, key, true).await;
    sqlx::query_scalar(
        "INSERT INTO social_produced_videos (twitch_user_id, clip_id, mode, status, file_path, duration_ms, expires_at, metadata)
         VALUES ($1, $2, $3, 'review', 'version.mp4', 15000, NOW() + INTERVAL '7 days', $4::text::jsonb) RETURNING id",
    )
    .bind(OWNER)
    .bind(clip)
    .bind(mode)
    .bind(metadata.to_string())
    .fetch_one(pool)
    .await
    .expect("Fassung")
}

async fn queue(pool: &PgPool, video: i64, platform: &str, status: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO twitch_clips_upload_queue (clip_id, produced_video_id, platform, status, scheduled_at, title)
         SELECT clip_id, id, $2, $3, NOW() + INTERVAL '1 day', 'Alt' FROM social_produced_videos WHERE id = $1 RETURNING id",
    )
    .bind(video)
    .bind(platform)
    .bind(status)
    .fetch_one(pool)
    .await
    .expect("Warteschlange")
}

async fn stored_platforms(pool: &PgPool, video: i64) -> Value {
    sqlx::query_scalar("SELECT metadata->'platforms' FROM social_produced_videos WHERE id = $1")
        .bind(video)
        .fetch_one(pool)
        .await
        .expect("Metadaten")
}

fn upload_metadata() -> Value {
    json!({
        "production_mode": "upload",
        "platforms": {"tiktok": {"title": "Standard", "description": "Folgt mir", "hashtags": ["deadlock"]}}
    })
}

#[tokio::test]
async fn defaults_survive_reload_and_version_edits_stay_explicit() {
    let (_db, pool) = database().await;
    let mut policy = autopilot_settings::load_policy(&pool, OWNER)
        .await
        .expect("Policy");
    policy.metadata_scope = PolicyScope::PerPlatform;
    policy.text_defaults.shared = TextDefaults {
        description: "Für alle".into(),
        hashtags: vec!["deadlock".into()],
    };
    policy.text_defaults.per_platform.insert(
        "tiktok".into(),
        TextDefaults {
            description: "Nur TikTok".into(),
            hashtags: vec!["fyp".into()],
        },
    );
    let saved = autopilot_settings::save_policy(&pool, OWNER, &policy)
        .await
        .expect("Policy speichern");
    let reloaded = autopilot_settings::load_policy(&pool, OWNER)
        .await
        .expect("Policy laden");
    assert_eq!(reloaded, saved);
    assert_eq!(reloaded.metadata_scope, PolicyScope::PerPlatform);
    assert_eq!(
        reloaded.text_defaults.per_platform["tiktok"].description,
        "Nur TikTok"
    );
    assert_eq!(reloaded.text_defaults.shared.description, "Für alle");

    let video = version(&pool, "upload", "static", upload_metadata()).await;
    let view = metadata::load(&pool, OWNER, video).await.expect("Ansicht");
    assert!(view.editable);
    assert_eq!(view.targets, vec!["tiktok"]);
    assert_eq!(view.platforms.len(), 3);
    assert_eq!(view.platforms["youtube"], view.platforms["tiktok"]);

    let edited = metadata::save(
        &pool,
        OWNER,
        video,
        all(copy(" Mein Titel ", "Eigener Text", &["#clip"])),
    )
    .await
    .expect("Speichern");
    assert_eq!(
        edited.platforms["tiktok"],
        copy("Mein Titel", "Eigener Text", &["clip"])
    );
    assert_eq!(
        stored_platforms(&pool, video).await,
        json!({"tiktok": {"title": "Mein Titel", "description": "Eigener Text", "hashtags": ["clip"]}})
    );

    let mut later = reloaded.clone();
    later.text_defaults.per_platform.insert(
        "tiktok".into(),
        TextDefaults {
            description: "Neu".into(),
            hashtags: vec![],
        },
    );
    autopilot_settings::save_policy(&pool, OWNER, &later)
        .await
        .expect("Policy ändern");
    let after = metadata::load(&pool, OWNER, video).await.expect("Ansicht");
    assert_eq!(after.platforms["tiktok"].description, "Eigener Text");
}

#[tokio::test]
async fn ai_and_text_versions_share_the_editor_contract() {
    let (_db, pool) = database().await;
    let ai = version(
        &pool,
        "ai",
        "ai",
        json!({"platforms": {
            "youtube": {"title": "KI", "description": "", "hashtags": []},
            "tiktok": {"title": "KI", "description": "", "hashtags": []},
            "instagram": {"title": "KI", "description": "KI", "hashtags": []}
        }}),
    )
    .await;
    let held = queue(&pool, ai, "youtube", "autopilot_held").await;
    let saved = metadata::save(&pool, OWNER, ai, all(copy("Neu", "Text", &["deadlock"])))
        .await
        .expect("KI speichern");
    assert_eq!(saved.targets, vec!["youtube", "tiktok", "instagram"]);
    let title: String =
        sqlx::query_scalar("SELECT title FROM twitch_clips_upload_queue WHERE id = $1")
            .bind(held)
            .fetch_one(&pool)
            .await
            .expect("Titel");
    assert_eq!(title, "Neu");

    let text = version(
        &pool,
        "text",
        "static",
        json!({"production_mode": "upload_text", "platforms": {
            "youtube": {"title": "Text", "description": "", "hashtags": []}
        }}),
    )
    .await;
    let mut input = all(copy("", "", &[]));
    input.insert("youtube".into(), copy("Text neu", "Mehr", &[]));
    let saved = metadata::save(&pool, OWNER, text, input)
        .await
        .expect("Text speichern");
    assert_eq!(saved.targets, vec!["youtube"]);
    assert_eq!(saved.platforms["youtube"].title, "Text neu");

    let mut missing = all(copy("Titel", "Text", &[]));
    missing.remove("youtube");
    assert!(matches!(
        metadata::save(&pool, OWNER, text, missing).await,
        Err(MetadataError::Invalid(_))
    ));
    assert!(matches!(
        metadata::save(&pool, "999", text, all(copy("Titel", "Text", &[]))).await,
        Err(MetadataError::Editor(AiEditorError::NotFound))
    ));
}

#[tokio::test]
async fn committed_or_started_payloads_reject_mutation() {
    let (_db, pool) = database().await;
    let cases = [
        ("effect", "UPDATE twitch_clips_upload_queue SET effect_started_at = NOW() WHERE id = $1"),
        ("external", "UPDATE twitch_clips_upload_queue SET external_id = 'ext-1' WHERE id = $1"),
        ("preparing", "UPDATE twitch_clips_upload_queue SET status = 'autopilot_preparing' WHERE id = $1"),
        ("published", "UPDATE twitch_clips_upload_queue SET status = 'completed' WHERE id = $1"),
        ("executed", "UPDATE social_produced_videos SET publication_executed_at = NOW() WHERE id = (SELECT produced_video_id FROM twitch_clips_upload_queue WHERE id = $1)"),
    ];
    for (key, mutation) in cases {
        let video = version(&pool, key, "static", upload_metadata()).await;
        let row = queue(&pool, video, "tiktok", "waiting_exact_file").await;
        assert!(
            metadata::load(&pool, OWNER, video)
                .await
                .expect("Ansicht")
                .editable
        );
        sqlx::query(mutation)
            .bind(row)
            .execute(&pool)
            .await
            .expect("Zustand");
        let view = metadata::load(&pool, OWNER, video).await.expect("Ansicht");
        assert!(!view.editable, "{key}");
        let result = metadata::save(&pool, OWNER, video, all(copy("Neu", "Neu", &[]))).await;
        assert!(
            matches!(result, Err(MetadataError::Editor(AiEditorError::Conflict))),
            "{key}"
        );
        assert_eq!(
            stored_platforms(&pool, video).await,
            upload_metadata()["platforms"],
            "{key}"
        );
        let title: String =
            sqlx::query_scalar("SELECT title FROM twitch_clips_upload_queue WHERE id = $1")
                .bind(row)
                .fetch_one(&pool)
                .await
                .expect("Titel");
        assert_eq!(title, "Alt", "{key}");
    }
}

#[tokio::test]
async fn legacy_templates_only_fill_empty_pool_copy() {
    let (_db, pool) = database().await;
    let template = clip_templates::create_global_template(
        &pool,
        "Standard",
        "{{streamer}} spielt {{game}}",
        &["#vorlage".to_string()],
        None,
        "admin",
    )
    .await
    .expect("Vorlage");
    let managed = clip(&pool, "managed", true).await;
    sqlx::query("UPDATE twitch_clips_social_media SET hashtags = '[\"#migriert\"]' WHERE id = $1")
        .bind(managed)
        .execute(&pool)
        .await
        .expect("Migrierte Hashtags");
    let legacy = clip(&pool, "legacy", false).await;
    sqlx::query("UPDATE twitch_clips_social_media SET custom_description = 'Alt', hashtags = '[\"#alt\"]' WHERE id = $1")
        .bind(legacy)
        .execute(&pool)
        .await
        .expect("Alte Texte");
    assert!(clip_templates::apply_template_to_clip(&pool, managed, template, true).await);
    assert!(clip_templates::apply_template_to_clip(&pool, legacy, template, true).await);
    let read = |id: i64| {
        let pool = pool.clone();
        async move {
            sqlx::query_as::<_, (Option<String>, Option<String>)>(
                "SELECT custom_description, hashtags FROM twitch_clips_social_media WHERE id = $1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("Clip lesen")
        }
    };
    let (description, hashtags) = read(managed).await;
    assert_eq!(description.as_deref(), Some("alpha spielt Deadlock"));
    assert_eq!(hashtags.as_deref(), Some("[\"#migriert\"]"));
    sqlx::query(
        "UPDATE twitch_clips_social_media SET custom_description = 'Eigener Text' WHERE id = $1",
    )
    .bind(managed)
    .execute(&pool)
    .await
    .expect("Eigener Text");
    assert!(clip_templates::apply_template_to_clip(&pool, managed, template, true).await);
    assert_eq!(read(managed).await.0.as_deref(), Some("Eigener Text"));
    let (description, hashtags) = read(legacy).await;
    assert_eq!(description.as_deref(), Some("alpha spielt Deadlock"));
    assert_eq!(hashtags.as_deref(), Some("[\"#vorlage\"]"));
}
