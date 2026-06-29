//! Frische Migrationen gegen eine leere Wegwerf-DB.
//! Ohne `TEST_DATABASE_URL` wird der Test laut uebersprungen.

use std::time::Duration;

use sqlx::postgres::PgPoolOptions;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

fn test_dsn() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

async fn column_type(pool: &sqlx::PgPool, table: &str, column: &str) -> String {
    sqlx::query_scalar(
        "SELECT data_type
           FROM information_schema.columns
          WHERE table_schema = 'public'
            AND table_name = $1
            AND column_name = $2",
    )
    .bind(table)
    .bind(column)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|err| panic!("column type for {table}.{column}: {err}"))
}

async fn column_nullable(pool: &sqlx::PgPool, table: &str, column: &str) -> bool {
    sqlx::query_scalar(
        "SELECT is_nullable = 'YES'
           FROM information_schema.columns
          WHERE table_schema = 'public'
            AND table_name = $1
            AND column_name = $2",
    )
    .bind(table)
    .bind(column)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|err| panic!("column nullable for {table}.{column}: {err}"))
}

async fn sequence_type(pool: &sqlx::PgPool, sequence: &str) -> Option<String> {
    sqlx::query_scalar(
        "SELECT format_type(seqtypid, NULL)
           FROM pg_sequence
          WHERE seqrelid = to_regclass($1)",
    )
    .bind(sequence)
    .fetch_optional(pool)
    .await
    .unwrap_or_else(|err| panic!("sequence type for {sequence}: {err}"))
}

#[tokio::test]
async fn fresh_migrations_apply_expected_analytics_schema_types() {
    let dsn = match test_dsn() {
        Some(dsn) => dsn,
        None => {
            eprintln!("SKIP: TEST_DATABASE_URL nicht gesetzt");
            return;
        }
    };

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&dsn)
        .await
        .expect("connect fresh test db");

    sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb")
        .execute(&pool)
        .await
        .expect("create timescaledb extension");

    MIGRATOR.run(&pool).await.expect("run all migrations");

    for (table, column, expected) in [
        (
            "clip_fetch_history",
            "fetched_at",
            "timestamp with time zone",
        ),
        ("clip_fetch_history", "id", "bigint"),
        (
            "clip_last_hashtags",
            "last_used_at",
            "timestamp with time zone",
        ),
        (
            "clip_templates_global",
            "created_at",
            "timestamp with time zone",
        ),
        ("clip_templates_global", "id", "bigint"),
        (
            "clip_templates_streamer",
            "created_at",
            "timestamp with time zone",
        ),
        ("clip_templates_streamer", "id", "bigint"),
        ("clip_templates_streamer", "is_default", "boolean"),
        (
            "clip_templates_streamer",
            "updated_at",
            "timestamp with time zone",
        ),
        ("twitch_ad_break_events", "id", "bigint"),
        ("twitch_ad_break_events", "session_id", "bigint"),
        (
            "twitch_ad_break_events",
            "started_at",
            "timestamp with time zone",
        ),
        ("twitch_ads_schedule_snapshot", "id", "bigint"),
        (
            "twitch_ads_schedule_snapshot",
            "last_ad_at",
            "timestamp with time zone",
        ),
        (
            "twitch_ads_schedule_snapshot",
            "next_ad_at",
            "timestamp with time zone",
        ),
        (
            "twitch_ads_schedule_snapshot",
            "snapshot_at",
            "timestamp with time zone",
        ),
        (
            "twitch_ads_schedule_snapshot",
            "snooze_refresh_at",
            "timestamp with time zone",
        ),
        ("twitch_ban_events", "ends_at", "timestamp with time zone"),
        ("twitch_ban_events", "id", "bigint"),
        (
            "twitch_ban_events",
            "received_at",
            "timestamp with time zone",
        ),
        ("twitch_ban_events", "session_id", "bigint"),
        ("twitch_bits_events", "id", "bigint"),
        (
            "twitch_bits_events",
            "received_at",
            "timestamp with time zone",
        ),
        ("twitch_bits_events", "session_id", "bigint"),
        ("twitch_channel_points_events", "id", "bigint"),
        (
            "twitch_channel_points_events",
            "redeemed_at",
            "timestamp with time zone",
        ),
        ("twitch_channel_points_events", "session_id", "bigint"),
        ("twitch_channel_updates", "id", "bigint"),
        (
            "twitch_channel_updates",
            "recorded_at",
            "timestamp with time zone",
        ),
        ("twitch_chat_messages", "id", "bigint"),
        ("twitch_clips_social_analytics", "clip_id", "bigint"),
        (
            "twitch_clips_social_analytics",
            "completion_rate",
            "double precision",
        ),
        ("twitch_clips_social_analytics", "ctr", "double precision"),
        (
            "twitch_clips_social_analytics",
            "engagement_rate",
            "double precision",
        ),
        ("twitch_clips_social_analytics", "id", "bigint"),
        (
            "twitch_clips_social_analytics",
            "posted_at",
            "timestamp with time zone",
        ),
        (
            "twitch_clips_social_analytics",
            "synced_at",
            "timestamp with time zone",
        ),
        (
            "twitch_clips_social_analytics",
            "watch_time_avg",
            "double precision",
        ),
        (
            "twitch_clips_social_media",
            "downloaded_at",
            "timestamp with time zone",
        ),
        (
            "twitch_clips_social_media",
            "duration_seconds",
            "double precision",
        ),
        ("twitch_clips_social_media", "id", "bigint"),
        (
            "twitch_clips_social_media",
            "instagram_uploaded_at",
            "timestamp with time zone",
        ),
        (
            "twitch_clips_social_media",
            "last_analytics_sync",
            "timestamp with time zone",
        ),
        (
            "twitch_clips_social_media",
            "tiktok_uploaded_at",
            "timestamp with time zone",
        ),
        ("twitch_clips_social_media", "uploaded_instagram", "boolean"),
        ("twitch_clips_social_media", "uploaded_tiktok", "boolean"),
        ("twitch_clips_social_media", "uploaded_youtube", "boolean"),
        (
            "twitch_clips_social_media",
            "youtube_uploaded_at",
            "timestamp with time zone",
        ),
        ("twitch_clips_upload_queue", "clip_id", "bigint"),
        (
            "twitch_clips_upload_queue",
            "completed_at",
            "timestamp with time zone",
        ),
        (
            "twitch_clips_upload_queue",
            "created_at",
            "timestamp with time zone",
        ),
        ("twitch_clips_upload_queue", "id", "bigint"),
        (
            "twitch_clips_upload_queue",
            "last_attempt_at",
            "timestamp with time zone",
        ),
        (
            "twitch_clips_upload_queue",
            "scheduled_at",
            "timestamp with time zone",
        ),
        ("twitch_eventsub_capacity_snapshot", "id", "bigint"),
        (
            "twitch_eventsub_capacity_snapshot",
            "ts_utc",
            "timestamp with time zone",
        ),
        (
            "twitch_eventsub_capacity_snapshot",
            "utilization_pct",
            "double precision",
        ),
        (
            "twitch_follow_events",
            "followed_at",
            "timestamp with time zone",
        ),
        ("twitch_follow_events", "id", "bigint"),
        (
            "twitch_hype_train_events",
            "ended_at",
            "timestamp with time zone",
        ),
        ("twitch_hype_train_events", "id", "bigint"),
        ("twitch_hype_train_events", "session_id", "bigint"),
        (
            "twitch_hype_train_events",
            "started_at",
            "timestamp with time zone",
        ),
        (
            "twitch_link_clicks",
            "clicked_at",
            "timestamp with time zone",
        ),
        ("twitch_link_clicks", "id", "bigint"),
        (
            "twitch_raid_auth",
            "authorized_at",
            "timestamp with time zone",
        ),
        ("twitch_raid_auth", "created_at", "timestamp with time zone"),
        (
            "twitch_raid_auth",
            "enc_migrated_at",
            "timestamp with time zone",
        ),
        (
            "twitch_raid_auth",
            "last_refreshed_at",
            "timestamp with time zone",
        ),
        (
            "twitch_raid_auth",
            "reauth_notified_at",
            "timestamp with time zone",
        ),
        (
            "twitch_raid_auth",
            "token_expires_at",
            "timestamp with time zone",
        ),
        ("twitch_shoutout_events", "id", "bigint"),
        (
            "twitch_shoutout_events",
            "received_at",
            "timestamp with time zone",
        ),
        ("twitch_stream_sessions", "dropoff_pct", "double precision"),
        (
            "twitch_stream_sessions",
            "retention_10m",
            "double precision",
        ),
        (
            "twitch_stream_sessions",
            "retention_20m",
            "double precision",
        ),
        ("twitch_stream_sessions", "retention_5m", "double precision"),
        ("twitch_streamers", "created_at", "timestamp with time zone"),
        ("twitch_subscription_events", "id", "bigint"),
        (
            "twitch_subscription_events",
            "received_at",
            "timestamp with time zone",
        ),
        ("twitch_subscription_events", "session_id", "bigint"),
        ("twitch_subscriptions_snapshot", "id", "bigint"),
        (
            "twitch_subscriptions_snapshot",
            "snapshot_at",
            "timestamp with time zone",
        ),
        ("twitch_stream_sessions", "id", "bigint"),
        ("twitch_stream_sessions", "avg_viewers", "double precision"),
        ("twitch_session_viewers", "session_id", "bigint"),
        (
            "twitch_session_viewers",
            "ts_utc",
            "timestamp with time zone",
        ),
        ("twitch_chat_messages", "session_id", "bigint"),
        (
            "twitch_chat_messages",
            "message_ts",
            "timestamp with time zone",
        ),
        ("twitch_raid_retention", "target_session_id", "bigint"),
    ] {
        let actual = column_type(&pool, table, column).await;
        assert_eq!(actual, expected, "{table}.{column}");
    }

    for (table, column) in [
        ("twitch_ads_schedule_snapshot", "snapshot_at"),
        ("twitch_eventsub_capacity_snapshot", "ts_utc"),
        ("twitch_hype_train_events", "started_at"),
        ("twitch_link_clicks", "clicked_at"),
        ("twitch_live_announcement_configs", "updated_at"),
        ("twitch_stats_category", "streamer"),
        ("twitch_stats_category", "ts_utc"),
        ("twitch_stats_tracked", "streamer"),
        ("twitch_stats_tracked", "ts_utc"),
        ("twitch_subscriptions_snapshot", "snapshot_at"),
    ] {
        let actual = column_nullable(&pool, table, column).await;
        assert!(!actual, "{table}.{column}");
    }

    for sequence in [
        "public.clip_fetch_history_id_seq",
        "public.clip_templates_global_id_seq",
        "public.clip_templates_streamer_id_seq",
        "public.twitch_ad_break_events_id_seq",
        "public.twitch_ads_schedule_snapshot_id_seq",
        "public.twitch_ban_events_id_seq",
        "public.twitch_bits_events_id_seq",
        "public.twitch_channel_points_events_id_seq",
        "public.twitch_channel_updates_id_seq",
        "public.twitch_chat_messages_id_seq",
        "public.twitch_clips_social_analytics_id_seq",
        "public.twitch_clips_social_media_id_seq",
        "public.twitch_clips_upload_queue_id_seq",
        "public.twitch_eventsub_capacity_snapshot_id_seq",
        "public.twitch_follow_events_id_seq",
        "public.twitch_hype_train_events_id_seq",
        "public.twitch_link_clicks_id_seq",
        "public.twitch_shoutout_events_id_seq",
        "public.twitch_stream_sessions_id_seq",
        "public.twitch_subscription_events_id_seq",
        "public.twitch_subscriptions_snapshot_id_seq",
    ] {
        if let Some(actual) = sequence_type(&pool, sequence).await {
            assert_eq!(actual, "bigint", "{sequence}");
        }
    }
}
