use std::str::FromStr;

use super::*;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

fn query(owner: Option<&str>, eligibility: Option<&str>) -> Query<SourcesQuery> {
    Query(SourcesQuery {
        twitch_user_id: owner.map(str::to_owned),
        limit: None,
        offset: None,
        eligibility: eligibility.map(str::to_owned),
    })
}

fn partner() -> DashboardAuthLevel {
    DashboardAuthLevel::Partner {
        twitch_login: "channel".into(),
        twitch_user_id: "42".into(),
        display_name: String::new(),
    }
}

async fn schema() -> PgPool {
    let dsn = std::env::var("TB_TEST_DATABASE_URL").expect("owned local test database required");
    let schema = "t_u4_pipeline_api";
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP SCHEMA IF EXISTS {schema} CASCADE"
    )))
    .execute(&admin)
    .await
    .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    let options = PgConnectOptions::from_str(&dsn)
        .unwrap()
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE TABLE twitch_streamers (twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT UNIQUE NOT NULL);
        INSERT INTO twitch_streamers VALUES ('channel', '42'), ('other', '99');
        CREATE TABLE social_media_partner_access (twitch_user_id TEXT PRIMARY KEY, granted BOOLEAN);
        INSERT INTO social_media_partner_access VALUES ('42', TRUE);
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
    for migration in [
        include_str!("../../../../migrations/20261011100000_social_autopilot_contracts.sql"),
        include_str!("../../../../migrations/20261011101000_social_clip_pool.sql"),
        include_str!("../../../../migrations/20261015124000_social_pipeline_policy.sql"),
        include_str!("../../../../migrations/20261015180000_social_destination_slots.sql"),
    ] {
        sqlx::raw_sql(migration).execute(&pool).await.unwrap();
    }
    sqlx::query("INSERT INTO twitch_clips_social_media
        (clip_id, twitch_user_id, streamer_login, clip_url, source_kind, category_key, game_id, created_at)
        VALUES ('manual', '42', 'channel', 'https://clips.test/manual', 'manual_upload', 'other', NULL, '2025-07-01T00:00:00Z'),
            ('foreign_game', '42', 'channel', 'https://clips.test/foreign', 'twitch', 'other', '1', '2025-08-01T00:00:00Z'),
            ('other_owner', '99', 'other', 'https://clips.test/other', 'twitch', 'other', '1', '2025-08-02T00:00:00Z')")
        .execute(&pool).await.unwrap();
    pool
}

async fn body(response: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn pipeline_sources_are_owner_scoped_and_keep_manual_and_legacy_rows_visible() {
    let pool = schema().await;
    assert_eq!(
        sources_handler(
            DashboardAuthLevel::None,
            State(pool.clone()),
            query(None, None)
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        sources_handler(partner(), State(pool.clone()), query(Some("99"), None))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        sources_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            query(None, None)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        sources_handler(partner(), State(pool.clone()), query(None, Some("bogus")))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    let response = sources_handler(partner(), State(pool.clone()), query(None, None)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let page = body(response).await;
    assert_eq!(page["total"], 2);
    assert_eq!(
        page["counts"],
        json!({"automatic": 0, "manual": 1, "legacy_non_deadlock_visible": 1})
    );
    let sources: Vec<(String, String, bool)> = page["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|source| {
            (
                source["clip_id"].as_str().unwrap().to_owned(),
                source["eligibility"].as_str().unwrap().to_owned(),
                source["auto_post"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        sources,
        [
            ("manual".into(), "manual".into(), false),
            (
                "foreign_game".into(),
                "legacy_non_deadlock_visible".into(),
                false
            )
        ]
    );
    let admin = body(
        sources_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            query(Some("99"), Some("legacy_non_deadlock_visible")),
        )
        .await,
    )
    .await;
    assert_eq!(admin["total"], 1);
    assert_eq!(admin["sources"][0]["clip_id"], "other_owner");
    sqlx::query(
        "UPDATE social_media_partner_access SET granted = FALSE WHERE twitch_user_id = '42'",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        sources_handler(partner(), State(pool), query(None, None))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
}
