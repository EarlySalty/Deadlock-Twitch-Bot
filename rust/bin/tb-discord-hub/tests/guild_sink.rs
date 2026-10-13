#[path = "../../../test-support/schema_sql.rs"]
mod test_sql;

mod test_database {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-support/database.rs"
    ));
}

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use std::str::FromStr;
use tb_discord_hub::guilds::{
    deactivate_member, mark_guild_removed, persist_setup, upsert_guild, PgSink, SetupPersist,
};
use tb_discord_hub::GuildEventSink;

macro_rules! pool_or_skip {
    ($schema:expr) => {{
        let Some(dsn) = test_database::database_url() else {
            if test_database::required() {
                panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
            }
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        pool_in_schema(&dsn, $schema).await
    }};
}

async fn pool_in_schema(dsn: &str, schema: &str) -> PgPool {
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(dsn)
        .await
        .unwrap();
    sqlx::query(crate::test_sql::drop_schema(schema, true))
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query(crate::test_sql::create_schema(schema, false))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    let opts = PgConnectOptions::from_str(dsn)
        .unwrap()
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .unwrap();
    for ddl in [
        r#"CREATE TABLE twitch_streamer_identities (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT,
            discord_user_id TEXT,
            discord_display_name TEXT,
            is_on_discord INTEGER,
            created_at TEXT,
            updated_at TEXT
        )"#,
        r#"CREATE TABLE discord_guilds (
            guild_id BIGINT PRIMARY KEY CHECK (guild_id > 0),
            name TEXT NOT NULL DEFAULT '',
            language TEXT NOT NULL DEFAULT 'en',
            announce_channel_id BIGINT,
            alert_channel_id BIGINT,
            streamer_role_id BIGINT,
            ping_role_id BIGINT,
            promo_invite_url TEXT,
            auto_partner_default BOOLEAN NOT NULL DEFAULT TRUE,
            cross_server_raids BOOLEAN NOT NULL DEFAULT TRUE,
            ai_features BOOLEAN NOT NULL DEFAULT FALSE,
            status TEXT NOT NULL DEFAULT 'active',
            installed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            removed_at TIMESTAMPTZ,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_by TEXT
        )"#,
        r#"CREATE TABLE streamer_guilds (
            twitch_user_id TEXT NOT NULL,
            guild_id BIGINT NOT NULL REFERENCES discord_guilds(guild_id) ON DELETE CASCADE,
            active BOOLEAN NOT NULL DEFAULT TRUE,
            deactivated_by TEXT,
            deactivated_at TIMESTAMPTZ,
            joined_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            source TEXT NOT NULL DEFAULT 'onboarding',
            PRIMARY KEY (twitch_user_id, guild_id)
        )"#,
    ] {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }
    pool
}

#[tokio::test]
async fn guild_upsert_remove_und_replay_sind_idempotent() {
    let pool = pool_or_skip!("hub_sink_guild");
    let sink = PgSink::new(pool.clone());
    sink.guild_added(42, "Alpha").await;
    sink.guild_added(42, "Alpha").await;
    let (name, status): (String, String) =
        sqlx::query_as("SELECT name, status FROM discord_guilds WHERE guild_id = 42")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(name, "Alpha");
    assert_eq!(status, "active");
    sink.guild_removed(42).await;
    sink.guild_removed(42).await;
    let (status, removed): (String, Option<String>) =
        sqlx::query_as("SELECT status, removed_at::text FROM discord_guilds WHERE guild_id = 42")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "removed");
    assert!(removed.is_some());
    sink.guild_added(42, "Alpha").await;
    let status: String =
        sqlx::query_scalar("SELECT status FROM discord_guilds WHERE guild_id = 42")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "active");
}

#[tokio::test]
async fn mitglied_verlassen_deaktiviert_zugehoerigkeit() {
    let pool = pool_or_skip!("hub_sink_leave");
    upsert_guild(&pool, 7, "Server").await.unwrap();
    sqlx::query(
        "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, discord_user_id)
         VALUES ('11', 'streamer', '99')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO streamer_guilds (twitch_user_id, guild_id, active, source)
         VALUES ('11', 7, TRUE, 'onboarding')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let sink = PgSink::new(pool.clone());
    sink.member_left(7, 99).await;
    sink.member_left(7, 99).await;
    let (active, by): (bool, Option<String>) = sqlx::query_as(
        "SELECT active, deactivated_by FROM streamer_guilds WHERE twitch_user_id = '11'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!active);
    assert_eq!(by.as_deref(), Some("discord_leave"));
    let _ = (
        mark_guild_removed(&pool, 7).await,
        deactivate_member(&pool, 7, 99).await,
    );
}

#[tokio::test]
async fn setup_optionen_werden_gespeichert() {
    let pool = pool_or_skip!("hub_sink_setup");
    persist_setup(
        &pool,
        21,
        "Foreign",
        &SetupPersist {
            language: Some("en".into()),
            announce_channel_id: Some(3),
            alert_channel_id: Some(4),
            streamer_role_id: Some(5),
            ping_role_id: Some(6),
            promo_invite_url: Some("https://discord.gg/abc".into()),
            auto_partner_default: Some(false),
        },
    )
    .await
    .unwrap();
    persist_setup(
        &pool,
        21,
        "Foreign",
        &SetupPersist {
            language: Some("en".into()),
            announce_channel_id: Some(3),
            alert_channel_id: Some(4),
            streamer_role_id: Some(5),
            ping_role_id: Some(6),
            promo_invite_url: Some("https://discord.gg/abc".into()),
            auto_partner_default: Some(false),
        },
    )
    .await
    .unwrap();
    let (language, auto_partner, invite): (String, bool, Option<String>) = sqlx::query_as(
        "SELECT language, auto_partner_default, promo_invite_url FROM discord_guilds WHERE guild_id = 21",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(language, "en");
    assert!(!auto_partner);
    assert_eq!(invite.as_deref(), Some("https://discord.gg/abc"));
    persist_setup(&pool, 21, "Foreign", &SetupPersist::default())
        .await
        .unwrap();
    let (language, auto_partner): (String, bool) = sqlx::query_as(
        "SELECT language, auto_partner_default FROM discord_guilds WHERE guild_id = 21",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(language, "en");
    assert!(!auto_partner);
}
