use std::str::FromStr;
use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_db::network::{
    community_settings, load_network, load_network_by_login, load_networks,
    record_broadcaster_languages, twitch_user_id_for_login,
};
use tb_domain::network::{raid_reach, RaidBlock, RaidReach, COMMUNITY_GUILD_ID};

#[path = "../../../test-support/database.rs"]
mod test_database;

static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

static MIGRATION_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();

const GUILD_MIGRATION: &str =
    include_str!("../../../migrations/20261012120000_multiserver_guilds_language.sql");

const EN_GUILD: i64 = 900_000_000_000_000_001;

#[tokio::test]
async fn upgrade_erhaelt_kanonische_partner_und_inaktive_metadaten() {
    let Some(pool) = migration_pool("tb_test_membership_v2_upgrade", true).await else {
        return;
    };
    exec(&pool, "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, manual_partner_opt_out, technical_pause_reason, admin_archived_at)
        SELECT n::text, 'legacy_' || n,
               CASE WHEN n > 73 THEN 'departnered' ELSE 'active' END,
               CASE WHEN n BETWEEN 62 AND 65 THEN 1 ELSE 0 END,
               CASE WHEN n BETWEEN 66 AND 69 THEN 'token_error' ELSE NULL END,
               CASE WHEN n BETWEEN 70 AND 73 THEN '2026-01-01T00:00:00Z' ELSE NULL END
        FROM generate_series(1, 77) n").await;
    {
        let _lock = MIGRATION_LOCK
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await;
        MIGRATOR
            .run(&pool)
            .await
            .expect("upgrade incoming migrations");
        MIGRATOR
            .run(&pool)
            .await
            .expect("idempotent migration ledger");
    }
    let counts: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(*) FILTER (WHERE active), COUNT(*) FILTER (WHERE NOT active AND deactivated_at IS NOT NULL), (SELECT COUNT(*) FROM partner_status_shadow_mismatch) FROM streamer_guilds",
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(counts, (77, 61, 16, 0));
    let states_preserved: bool = sqlx::query_scalar("SELECT NOT EXISTS (SELECT 1 FROM twitch_partners_all_state n JOIN twitch_partners_legacy_state o USING (id) WHERE n.operational_state IS DISTINCT FROM o.operational_state)")
        .fetch_one(&pool).await.unwrap();
    assert!(states_preserved);
    let unique: bool =
        sqlx::query_scalar("SELECT COUNT(*) = COUNT(DISTINCT version) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(unique);
    exec(&pool, "UPDATE streamer_guilds SET active = FALSE, deactivated_by = 'discord:admin', deactivated_at = NOW() WHERE twitch_user_id = '1'; UPDATE discord_guilds SET status = 'removed', removed_at = NOW(), ai_features = FALSE, language = 'en', updated_by = 'discord:admin';").await;
    exec(&pool, GUILD_MIGRATION).await;
    exec(
        &pool,
        include_str!("../../../migrations/20261013120000_guilds_partner_membership.sql"),
    )
    .await;
    let unchanged: bool = sqlx::query_scalar("SELECT NOT sg.active AND sg.deactivated_by = 'discord:admin' AND g.status = 'removed' AND g.removed_at IS NOT NULL AND NOT g.ai_features AND g.language = 'en' FROM streamer_guilds sg JOIN discord_guilds g USING (guild_id) WHERE sg.twitch_user_id = '1'").fetch_one(&pool).await.unwrap();
    assert!(unchanged);
}

#[tokio::test]
async fn kanonischer_partner_und_ki_guard_folgen_installierten_guilds() {
    let Some(pool) = migrated_pool("tb_test_membership_v2_transitions").await else {
        return;
    };
    exec(&pool, "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status) VALUES ('8001', 'verified_test', 'active'); INSERT INTO discord_guilds (guild_id, language, ai_features) VALUES (8002, 'en', FALSE);").await;
    async fn state(pool: &PgPool) -> (i32, bool) {
        sqlx::query_as("SELECT is_partner_active, twitch_streamer_ai_allowed('8001', 'verified_test') FROM twitch_streamers_partner_state WHERE twitch_user_id = '8001'").fetch_one(pool).await.unwrap()
    }
    assert_eq!(state(&pool).await, (0, false));
    exec(&pool, &format!("INSERT INTO streamer_guilds (twitch_user_id, guild_id, active, source) VALUES ('8001', {COMMUNITY_GUILD_ID}, TRUE, 'onboarding')")).await;
    assert_eq!(state(&pool).await, (1, true));
    exec(&pool, "INSERT INTO streamer_guilds (twitch_user_id, guild_id, active, source) VALUES ('8001', 8002, TRUE, 'onboarding')").await;
    assert_eq!(state(&pool).await, (1, false));
    exec(
        &pool,
        "UPDATE streamer_guilds SET active = FALSE, deactivated_at = NOW() WHERE guild_id = 8002",
    )
    .await;
    assert_eq!(state(&pool).await, (1, true));
    exec(
        &pool,
        "UPDATE discord_guilds SET removed_at = NOW() WHERE language = 'de'",
    )
    .await;
    assert_eq!(state(&pool).await, (0, false));
    exec(&pool, "UPDATE discord_guilds SET removed_at = NULL WHERE language = 'de'; UPDATE twitch_partners SET manual_partner_opt_out = 1 WHERE twitch_user_id = '8001'").await;
    assert_eq!(state(&pool).await, (0, false));
    exec(&pool, "UPDATE twitch_partners SET manual_partner_opt_out = 0 WHERE twitch_user_id = '8001'; INSERT INTO twitch_partner_signup_denylist (twitch_user_id, twitch_login, reason, added_by) VALUES ('8001', 'verified_test', 'test', 'test')").await;
    assert_eq!(state(&pool).await, (0, false));
    exec(
        &pool,
        "DELETE FROM twitch_partner_signup_denylist WHERE twitch_user_id = '8001'",
    )
    .await;
    let denied: (bool, bool, bool) = sqlx::query_as("SELECT twitch_streamer_ai_allowed(NULL, 'historical_unknown'), twitch_streamer_ai_allowed('missing', NULL), twitch_streamer_ai_allowed('8001', 'wrong_login')").fetch_one(&pool).await.unwrap();
    assert_eq!(denied, (false, false, false));
    exec(&pool, "INSERT INTO twitch_exclusions (twitch_user_id, kind, reason) VALUES ('8001', 'banned', 'test')").await;
    assert_eq!(state(&pool).await, (0, false));
}

const EN_GUILD_OHNE_NETZWERK: i64 = 900_000_000_000_000_002;

fn test_dsn() -> Option<String> {
    let dsn = test_database::database_url();
    assert!(
        dsn.is_some() || !test_database::required(),
        "Test-Datenbank verlangt, aber kein DSN gesetzt"
    );
    dsn
}

async fn migrated_pool(db_name: &str) -> Option<PgPool> {
    migration_pool(db_name, false).await
}

async fn migration_pool(db_name: &str, legacy_only: bool) -> Option<PgPool> {
    let dsn = test_dsn()?;
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&dsn)
        .await
        .expect("admin connect");
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE IF EXISTS {db_name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {db_name}")))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;

    let opts = PgConnectOptions::from_str(&dsn)
        .expect("dsn parse")
        .database(db_name);
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(opts)
        .await
        .expect("connect");
    sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb")
        .execute(&pool)
        .await
        .ok();
    {
        let _nacheinander = MIGRATION_LOCK
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await;
        if legacy_only {
            let mut legacy = Migrator::new(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations"),
            )
            .await
            .unwrap();
            legacy.migrations = std::borrow::Cow::Owned(
                legacy
                    .iter()
                    .filter(|m| m.version < 20261012120000)
                    .cloned()
                    .collect(),
            );
            legacy.run(&pool).await.expect("run legacy migrations");
        } else {
            MIGRATOR.run(&pool).await.expect("run all migrations");
        }
    }
    Some(pool)
}

async fn exec(pool: &PgPool, sql: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(pool)
        .await
        .unwrap_or_else(|error| panic!("SQL fehlgeschlagen: {error}\n{sql}"));
}

async fn fremde_server_anlegen(pool: &PgPool) {
    exec(
        pool,
        &format!(
            "INSERT INTO discord_guilds (guild_id, name, language, promo_invite_url, cross_server_raids)
             VALUES ({EN_GUILD}, 'English Deadlock', 'en', 'https://discord.gg/english', TRUE),
                    ({EN_GUILD_OHNE_NETZWERK}, 'Closed Deadlock', 'en-gb', NULL, FALSE)"
        ),
    )
    .await;
}

#[tokio::test]
async fn migration_legt_deutschen_server_an_und_ordnet_aktive_partner_zu() {
    let Some(pool) = migrated_pool("tb_test_guilds_backfill").await else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };

    let (language, invite, cross, ai, status): (String, Option<String>, bool, bool, String) =
        sqlx::query_as(
            "SELECT language, promo_invite_url, cross_server_raids, ai_features, status
               FROM discord_guilds WHERE guild_id = $1",
        )
        .bind(COMMUNITY_GUILD_ID)
        .fetch_one(&pool)
        .await
        .expect("deutscher Server angelegt");
    assert_eq!(language, "de");
    assert_eq!(invite.as_deref(), Some("https://discord.gg/z5TfVHuQq2"));
    assert!(cross);
    assert!(ai);
    assert_eq!(status, "active");

    exec(
        &pool,
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status) VALUES
            ('100', 'aktiv_de', 'active'),
            ('200', 'ehemalig', 'departnered'),
            ('300', 'aktiv_en_kanal', 'active')",
    )
    .await;
    exec(
        &pool,
        "INSERT INTO twitch_channel_updates (twitch_user_id, language, recorded_at) VALUES
            ('300', 'de', now() - interval '3 days'),
            ('300', 'EN ', now() - interval '1 day')",
    )
    .await;
    exec(
        &pool,
        "INSERT INTO twitch_stream_sessions (twitch_user_id, streamer_login, language, started_at) VALUES
            ('100', 'aktiv_de', 'de', now() - interval '2 days'),
            ('100', 'aktiv_de', '', now() - interval '1 hour'),
            ('300', 'aktiv_en_kanal', 'de', now() - interval '2 days')",
    )
    .await;
    exec(&pool, GUILD_MIGRATION).await;
    exec(&pool, GUILD_MIGRATION).await;

    let rows: Vec<(String, i64, bool, String)> = sqlx::query_as(
        "SELECT twitch_user_id, guild_id, active, source FROM streamer_guilds ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![
            (
                "100".to_string(),
                COMMUNITY_GUILD_ID,
                true,
                "migration".to_string()
            ),
            (
                "200".to_string(),
                COMMUNITY_GUILD_ID,
                false,
                "migration".to_string()
            ),
            (
                "300".to_string(),
                COMMUNITY_GUILD_ID,
                true,
                "migration".to_string()
            ),
        ]
    );

    let languages: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT twitch_user_id, broadcaster_language FROM twitch_streamer_identities
          WHERE twitch_user_id IN ('100', '300') ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        languages,
        vec![
            ("100".to_string(), Some("de".to_string())),
            ("300".to_string(), Some("en".to_string())),
        ]
    );
}

#[tokio::test]
async fn netzwerk_ohne_zeile_bleibt_geschlossen() {
    let Some(pool) = migrated_pool("tb_test_guilds_netzwerk").await else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };
    fremde_server_anlegen(&pool).await;
    exec(
        &pool,
        &format!(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, broadcaster_language) VALUES
                ('de1', 'deutsch_ohne_zeile', NULL),
                ('en1', 'english_one', 'en'),
                ('en2', 'english_two', NULL),
                ('zu', 'closed_one', 'en');
             INSERT INTO streamer_guilds (twitch_user_id, guild_id, active, source, joined_at) VALUES
                ('en1', {EN_GUILD}, TRUE, 'onboarding', now() - interval '2 days'),
                ('en2', {EN_GUILD}, TRUE, 'onboarding', now() - interval '1 day'),
                ('zu', {EN_GUILD_OHNE_NETZWERK}, TRUE, 'onboarding', now())"
        ),
    )
    .await;

    let ids = ["de1", "en1", "en2", "zu", "unbekannt"].map(str::to_string);
    let networks = load_networks(&pool, &ids).await.expect("Netzwerke laden");
    assert_eq!(networks.len(), 5);

    let de = &networks["de1"];
    assert_eq!(de.home_guild().map(|g| g.guild_id), None);
    assert_eq!(de.language_group(None), None);
    let unbekannt = &networks["unbekannt"];
    assert_eq!(unbekannt.home_guild().map(|g| g.guild_id), None);

    let en1 = &networks["en1"];
    let home = en1.home_guild().expect("Heimat-Server");
    assert_eq!(home.guild_id, EN_GUILD);
    assert_eq!(home.promo_invite(), Some("https://discord.gg/english"));
    assert_eq!(en1.broadcaster_language(), Some("en"));

    let en2 = &networks["en2"];
    assert_eq!(en2.language_group(None).as_deref(), Some("en"));
    assert_eq!(
        raid_reach(en1, Some("en"), en2, Some("en")),
        Ok(RaidReach::SameServer)
    );
    assert_eq!(
        raid_reach(de, Some("de"), en1, Some("en")),
        Err(RaidBlock::NetworkSwitchOff)
    );
    let zu = &networks["zu"];
    assert_eq!(zu.language_group(None).as_deref(), Some("en"));
    assert_eq!(
        raid_reach(en1, Some("en"), zu, Some("en")),
        Err(RaidBlock::NetworkSwitchOff)
    );

    let einzeln = load_network(&pool, " en1 ").await.unwrap();
    assert_eq!(einzeln.home_guild().map(|g| g.guild_id), Some(EN_GUILD));
    let per_login = load_network_by_login(&pool, "English_One").await.unwrap();
    assert_eq!(per_login.home_guild().map(|g| g.guild_id), Some(EN_GUILD));
    let fremder_login = load_network_by_login(&pool, "niemand").await.unwrap();
    assert_eq!(fremder_login.home_guild().map(|g| g.guild_id), None);
    assert_eq!(
        twitch_user_id_for_login(&pool, " ENGLISH_TWO ")
            .await
            .unwrap(),
        Some("en2".to_string())
    );
}

#[tokio::test]
async fn deaktivierte_zeile_und_entfernter_server_zaehlen_nicht() {
    let Some(pool) = migrated_pool("tb_test_guilds_inaktiv").await else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };
    fremde_server_anlegen(&pool).await;
    exec(
        &pool,
        &format!(
            "UPDATE discord_guilds SET status = 'removed', removed_at = now()
              WHERE guild_id = {EN_GUILD_OHNE_NETZWERK};
             INSERT INTO streamer_guilds
                (twitch_user_id, guild_id, active, source, joined_at, deactivated_at, deactivated_by) VALUES
                ('wechsel', {COMMUNITY_GUILD_ID}, FALSE, 'migration', now() - interval '9 days', now(), 'admin:test'),
                ('wechsel', {EN_GUILD}, TRUE, 'onboarding', now() - interval '1 day', NULL, NULL),
                ('weg', {EN_GUILD_OHNE_NETZWERK}, TRUE, 'onboarding', now(), NULL, NULL)"
        ),
    )
    .await;

    let networks = load_networks(&pool, &["wechsel".to_string(), "weg".to_string()])
        .await
        .unwrap();
    assert_eq!(
        networks["wechsel"].home_guild().map(|g| g.guild_id),
        Some(EN_GUILD)
    );
    assert!(networks["weg"].home_guild().is_none());
    assert!(!networks["weg"].allows_cross_server_raids());
}

#[tokio::test]
async fn kanalsprache_wird_nur_bei_aenderung_geschrieben() {
    let Some(pool) = migrated_pool("tb_test_guilds_sprache").await else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };
    exec(
        &pool,
        "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login) VALUES
            ('1', 'eins'), ('2', 'zwei')",
    )
    .await;

    let observed = vec![
        ("1".to_string(), "DE".to_string()),
        ("2".to_string(), "en".to_string()),
        ("3".to_string(), "fr".to_string()),
        ("2".to_string(), " ".to_string()),
    ];
    assert_eq!(
        record_broadcaster_languages(&pool, &observed)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        record_broadcaster_languages(&pool, &observed)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        record_broadcaster_languages(&pool, &[("2".to_string(), "es".to_string())])
            .await
            .unwrap(),
        1
    );

    let rows: Vec<(String, Option<String>, bool)> = sqlx::query_as(
        "SELECT twitch_user_id, broadcaster_language, broadcaster_language_updated_at IS NOT NULL
           FROM twitch_streamer_identities ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![
            ("1".to_string(), Some("de".to_string()), true),
            ("2".to_string(), Some("es".to_string()), true),
        ]
    );
}

#[tokio::test]
async fn ohne_zeile_des_deutschen_servers_gelten_standardwerte() {
    let Some(pool) = migrated_pool("tb_test_guilds_standard").await else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };
    exec(
        &pool,
        "DELETE FROM streamer_guilds; DELETE FROM discord_guilds;",
    )
    .await;
    let community = community_settings(&pool).await.unwrap();
    assert_eq!(community.guild_id, COMMUNITY_GUILD_ID);
    assert_eq!(community.language, "de");
    assert!(community.cross_server_raids);
    assert!(!community.installed);
    assert_eq!(community.promo_invite(), None);
    assert!(load_networks(&pool, &[]).await.unwrap().is_empty());
}
