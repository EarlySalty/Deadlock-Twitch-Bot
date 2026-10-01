use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::str::FromStr;

#[path = "../../../test-support/database.rs"]
mod test_database;

#[tokio::test]
async fn backfill_preserves_unresolved_and_enforces_identity() {
    let Some(dsn) = test_database::database_url() else {
        assert!(!test_database::required(), "Isolierte Testdatenbank fehlt");
        return;
    };
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .unwrap();
    let schema = "t_sm_id_migration";
    sqlx::raw_sql(
        "DROP SCHEMA IF EXISTS t_sm_id_migration CASCADE; CREATE SCHEMA t_sm_id_migration",
    )
    .execute(&admin)
    .await
    .unwrap();
    let options = PgConnectOptions::from_str(&dsn)
        .unwrap()
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE twitch_streamers (twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT UNIQUE);
         INSERT INTO twitch_streamers VALUES ('earlysalty', '1186925760'), ('dach_lock', '1367527782'), ('unresolved', NULL), ('ambiguous', '100'), ('Ambiguous', '101');
         CREATE TABLE social_media_partner_access (
            streamer_login TEXT PRIMARY KEY REFERENCES twitch_streamers(twitch_login) ON DELETE CASCADE,
            twitch_user_id TEXT, granted BOOLEAN NOT NULL, granted_by TEXT, granted_at TIMESTAMPTZ NOT NULL DEFAULT NOW());
         CREATE UNIQUE INDEX social_media_partner_access_identity ON social_media_partner_access(twitch_user_id) WHERE twitch_user_id IS NOT NULL;
         INSERT INTO social_media_partner_access (streamer_login, granted, granted_by) VALUES ('earlysalty', TRUE, 'admin'), ('dach_lock', TRUE, 'admin'), ('unresolved', TRUE, 'legacy');
         CREATE TABLE clip_templates_streamer (streamer_login TEXT, twitch_user_id TEXT, template_name TEXT, CONSTRAINT clip_templates_streamer_streamer_login_template_name_key UNIQUE(streamer_login, template_name));
         CREATE TABLE clip_last_hashtags (streamer_login TEXT PRIMARY KEY, twitch_user_id TEXT);
         CREATE TABLE social_media_streamer_layout (streamer_login TEXT PRIMARY KEY REFERENCES twitch_streamers(twitch_login), twitch_user_id TEXT);
         CREATE TABLE social_media_streamer_settings (streamer_login TEXT PRIMARY KEY);
         INSERT INTO social_media_streamer_settings VALUES ('earlysalty'), ('ambiguous');
         CREATE TABLE social_media_platform_schedule (streamer_login TEXT, platform TEXT, PRIMARY KEY(streamer_login, platform));
         CREATE TABLE social_media_category (category_key TEXT PRIMARY KEY);
         INSERT INTO social_media_category VALUES ('deadlock');
         CREATE TABLE social_media_category_settings (streamer_login TEXT, category_key TEXT REFERENCES social_media_category(category_key), PRIMARY KEY(streamer_login, category_key));
         INSERT INTO social_media_category_settings VALUES ('earlysalty', 'deadlock');
         CREATE TABLE social_media_vod_archive (streamer_login TEXT PRIMARY KEY);
         CREATE TABLE twitch_vod_archive_vods (id BIGSERIAL PRIMARY KEY, streamer_login TEXT);
         INSERT INTO twitch_vod_archive_vods (streamer_login) VALUES ('earlysalty'), ('ambiguous');"
    ).execute(&pool).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/20261001090000_social_media_partner_ids_required.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    for table in [
        "social_media_streamer_settings",
        "social_media_category_settings",
        "twitch_vod_archive_vods",
    ] {
        let id: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT twitch_user_id FROM {table} WHERE streamer_login = 'earlysalty'"
        )))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(id, "1186925760");
    }
    assert!(sqlx::query("INSERT INTO social_media_category_settings (streamer_login, twitch_user_id, category_key) VALUES ('dach_lock', '1367527782', 'unknown')")
        .execute(&pool).await.is_err());
    let ambiguous_id: Option<String> = sqlx::query_scalar(
        "SELECT twitch_user_id FROM social_media_streamer_settings WHERE streamer_login = 'ambiguous'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(ambiguous_id.is_none());
    let ambiguous_vod_id: Option<String> = sqlx::query_scalar(
        "SELECT twitch_user_id FROM twitch_vod_archive_vods WHERE streamer_login = 'ambiguous'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(ambiguous_vod_id.is_none());
    let grants: Vec<(String, String, bool)> = sqlx::query_as("SELECT streamer_login, twitch_user_id, granted FROM social_media_partner_access ORDER BY streamer_login")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(
        grants,
        vec![
            ("dach_lock".into(), "1367527782".into(), true),
            ("earlysalty".into(), "1186925760".into(), true)
        ]
    );
    let unresolved: (String, bool, String) = sqlx::query_as(
        "SELECT streamer_login, granted, granted_by FROM social_media_partner_access_unresolved",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(unresolved, ("unresolved".into(), true, "legacy".into()));
    for statement in [
        "INSERT INTO social_media_partner_access (streamer_login, granted) VALUES ('missing', TRUE)",
        "INSERT INTO social_media_partner_access (streamer_login, twitch_user_id, granted) VALUES ('other', '1186925760', TRUE)",
        "INSERT INTO social_media_partner_access (streamer_login, twitch_user_id, granted) VALUES ('invalid', 'earlysalty', TRUE)",
    ] {
        assert!(sqlx::query(statement).execute(&pool).await.is_err());
    }
    sqlx::query(
        "UPDATE twitch_streamers SET twitch_user_id = '99' WHERE twitch_login = 'earlysalty'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let granted: bool = sqlx::query_scalar(
        "SELECT granted FROM social_media_partner_access WHERE twitch_user_id = '1186925760'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(granted);
    let new_owner: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM social_media_partner_access WHERE twitch_user_id = '99')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!new_owner);
    pool.close().await;
    sqlx::query("DROP SCHEMA t_sm_id_migration CASCADE")
        .execute(&admin)
        .await
        .unwrap();
}
