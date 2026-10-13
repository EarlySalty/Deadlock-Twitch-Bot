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
mod referral_test_support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-support/streamer_referrals.rs"
    ));
}

use tb_raid::partner_setup::{
    attach_streamer_guild, promote_streamer_to_partner, PromotePartnerArgs, COMMUNITY_GUILD_ID,
    STREAMER_GUILD_SOURCE_ONBOARDING,
};

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
    apply_ddl(&pool).await;
    pool
}

async fn apply_ddl(pool: &PgPool) {
    for ddl in [
        r#"CREATE TABLE twitch_partners (
            id BIGSERIAL PRIMARY KEY,
            twitch_user_id TEXT,
            twitch_login TEXT,
            require_discord_link INTEGER,
            last_description TEXT,
            last_link_ok INTEGER,
            added_by TEXT,
            last_link_checked_at TEXT,
            next_link_check_at TEXT,
            manual_partner_opt_out INTEGER,
            raid_admin_enabled BOOLEAN NOT NULL DEFAULT TRUE,
            raid_bot_enabled INTEGER,
            silent_ban INTEGER,
            silent_raid INTEGER,
            live_ping_role_id BIGINT,
            live_ping_enabled INTEGER,
            partnered_at TEXT,
            departnered_at TEXT,
            status TEXT,
            admin_archived_at TEXT,
            technical_pause_reason TEXT
        )"#,
        r#"CREATE TABLE twitch_streamer_identities (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT,
            discord_user_id TEXT,
            discord_display_name TEXT,
            is_on_discord INTEGER,
            created_at TEXT,
            updated_at TEXT
        )"#,
        r#"CREATE VIEW twitch_partners_all_state AS
            SELECT p.twitch_user_id,
                   CASE
                       WHEN p.status = 'active'
                        AND COALESCE(p.manual_partner_opt_out, 0) = 0
                        AND COALESCE(p.technical_pause_reason, '') = ''
                        AND p.admin_archived_at IS NULL
                       THEN 1 ELSE 0
                   END AS is_partner_active
              FROM twitch_partners p"#,
        r#"CREATE TABLE twitch_streamers (
            id BIGSERIAL PRIMARY KEY,
            twitch_login TEXT UNIQUE NOT NULL,
            twitch_user_id TEXT,
            created_at TIMESTAMPTZ DEFAULT NOW()
        )"#,
        r#"CREATE TABLE streamer_plans (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT,
            first_login_at TEXT
        )"#,
        r#"CREATE TABLE twitch_raid_auth (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT
        )"#,
        r#"CREATE TABLE twitch_partner_signup_denylist (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT NOT NULL,
            reason TEXT NOT NULL,
            public_message TEXT,
            added_by TEXT NOT NULL,
            added_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
        r#"CREATE TABLE twitch_raid_blacklist (
            target_login TEXT PRIMARY KEY,
            target_id TEXT,
            reason TEXT,
            added_at TEXT
        )"#,
        r#"CREATE TABLE twitch_partner_raid_scores (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT
        )"#,
        r#"CREATE TABLE twitch_live_state (
            twitch_user_id TEXT PRIMARY KEY,
            streamer_login TEXT NOT NULL
        )"#,
        r#"CREATE TABLE twitch_stats_category (
            ts_utc TIMESTAMPTZ NOT NULL,
            streamer TEXT NOT NULL
        )"#,
        r#"CREATE TABLE twitch_stats_tracked (
            ts_utc TIMESTAMPTZ NOT NULL,
            streamer TEXT NOT NULL
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
        r#"CREATE VIEW partner_guild_active AS
            SELECT DISTINCT sg.twitch_user_id
              FROM streamer_guilds sg
              JOIN discord_guilds dg ON dg.guild_id = sg.guild_id
             WHERE sg.active AND dg.status = 'active'"#,
        r#"CREATE VIEW partner_status_shadow_mismatch AS
            SELECT
                COALESCE(old_state.twitch_user_id, guild_state.twitch_user_id) AS twitch_user_id,
                COALESCE(old_state.is_partner_active, 0) AS old_is_partner_active,
                CASE WHEN guild_state.twitch_user_id IS NULL THEN 0 ELSE 1 END AS new_is_partner_active
            FROM (
                SELECT twitch_user_id, MAX(COALESCE(is_partner_active, 0)) AS is_partner_active
                  FROM twitch_partners_all_state
                 WHERE twitch_user_id ~ '^[0-9]{1,20}$'
                 GROUP BY twitch_user_id
            ) old_state
            FULL OUTER JOIN partner_guild_active guild_state
                ON guild_state.twitch_user_id = old_state.twitch_user_id
            WHERE COALESCE(old_state.is_partner_active, 0)
                  IS DISTINCT FROM CASE WHEN guild_state.twitch_user_id IS NULL THEN 0 ELSE 1 END"#,
    ] {
        sqlx::query(ddl).execute(pool).await.unwrap();
    }
    referral_test_support::schema(pool).await;
}

fn args(login: &str, uid: &str) -> PromotePartnerArgs {
    PromotePartnerArgs {
        twitch_login: login.to_string(),
        twitch_user_id: uid.to_string(),
        discord_user_id: None,
        discord_display_name: None,
        is_on_discord: 0,
        activate_partner_features: true,
        clear_source: true,
        guild_id: None,
    }
}

async fn insert_partner(
    pool: &PgPool,
    uid: &str,
    login: &str,
    status: &str,
    opt_out: i32,
    pause: Option<&str>,
    archived: bool,
) {
    sqlx::query(
        "INSERT INTO twitch_partners (
            twitch_user_id, twitch_login, status, manual_partner_opt_out,
            technical_pause_reason, admin_archived_at
         ) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(uid)
    .bind(login)
    .bind(status)
    .bind(opt_out)
    .bind(pause)
    .bind(archived.then_some("2026-01-01T00:00:00Z"))
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn shadow_vergleich_ist_leer_fuer_heutige_partnerzustaende() {
    let pool = pool_or_skip!("gm_shadow");
    insert_partner(&pool, "11", "aktiv", "active", 0, None, false).await;
    insert_partner(
        &pool,
        "12",
        "pause",
        "active",
        0,
        Some("token_error"),
        false,
    )
    .await;
    insert_partner(&pool, "13", "archiv", "archived", 0, None, true).await;
    insert_partner(&pool, "14", "optout", "active", 1, None, false).await;
    sqlx::query(
        "INSERT INTO discord_guilds (guild_id, name, language, status)
         VALUES ($1, 'Deutsche Deadlock Community', 'de', 'active')",
    )
    .bind(COMMUNITY_GUILD_ID)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO streamer_guilds (twitch_user_id, guild_id, active, source)
         SELECT p.twitch_user_id, $1,
                (p.status = 'active'
                 AND COALESCE(p.manual_partner_opt_out, 0) = 0
                 AND COALESCE(p.technical_pause_reason, '') = ''
                 AND p.admin_archived_at IS NULL),
                'migration'
           FROM twitch_partners p",
    )
    .bind(COMMUNITY_GUILD_ID)
    .execute(&pool)
    .await
    .unwrap();
    let mismatches: Vec<String> =
        sqlx::query_scalar("SELECT twitch_user_id FROM partner_status_shadow_mismatch")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(mismatches.is_empty(), "{mismatches:?}");
}

#[tokio::test]
async fn auto_partner_aus_bleibt_inaktiv() {
    let pool = pool_or_skip!("gm_inactive");
    sqlx::query(
        "INSERT INTO discord_guilds (guild_id, name, language, auto_partner_default, status)
         VALUES (99, 'Fremd', 'en', FALSE, 'active')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    attach_streamer_guild(&mut tx, "55", 99, false, STREAMER_GUILD_SOURCE_ONBOARDING)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let active: bool =
        sqlx::query_scalar("SELECT active FROM streamer_guilds WHERE twitch_user_id = '55'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!active);
}

#[tokio::test]
async fn promote_haengt_community_guild_aktiv() {
    let pool = pool_or_skip!("gm_promote");
    let mut tx = pool.begin().await.unwrap();
    promote_streamer_to_partner(&mut tx, &args("neu", "77"), chrono::Utc::now())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (guild_id, active): (i64, bool) =
        sqlx::query_as("SELECT guild_id, active FROM streamer_guilds WHERE twitch_user_id = '77'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(guild_id, COMMUNITY_GUILD_ID);
    assert!(active);
}

#[tokio::test]
async fn promote_mit_guild_id_haengt_gewaehlten_server() {
    let pool = pool_or_skip!("gm_guild_arg");
    sqlx::query(
        "INSERT INTO discord_guilds (guild_id, name, language, status)
         VALUES (88, 'Chosen', 'en', 'active')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let mut chosen = args("wahl", "88");
    chosen.guild_id = Some(88);
    let mut tx = pool.begin().await.unwrap();
    promote_streamer_to_partner(&mut tx, &chosen, chrono::Utc::now())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (guild_id, active): (i64, bool) =
        sqlx::query_as("SELECT guild_id, active FROM streamer_guilds WHERE twitch_user_id = '88'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(guild_id, 88);
    assert!(active);
}
