#[path = "../../../test-support/postgres.rs"]
mod test_postgres;
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
            technical_pause_reason TEXT,
            verified BOOLEAN NOT NULL DEFAULT FALSE,
            inactivity_flagged_at TEXT
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
            PRIMARY KEY (twitch_user_id, guild_id),
            CHECK (active OR deactivated_at IS NOT NULL)
        )"#,
        r#"CREATE TABLE twitch_exclusions (
            twitch_user_id TEXT PRIMARY KEY, kind TEXT NOT NULL, reactivated_at TIMESTAMPTZ
        )"#,
    ] {
        sqlx::query(ddl).execute(pool).await.unwrap();
    }
    let contract =
        include_str!("../../../migrations/20261013150100_multiserver_partner_contract.sql")
            .replace("public.", "");
    sqlx::raw_sql(sqlx::AssertSqlSafe(contract))
        .execute(pool)
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(
        "CREATE VIEW twitch_partners_all_state AS SELECT * FROM twitch_partners_legacy_state",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    let cutover =
        include_str!("../../../migrations/20261013150300_multiserver_partner_cutover.sql")
            .replace("public.", "");
    sqlx::raw_sql(sqlx::AssertSqlSafe(cutover))
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
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
        "INSERT INTO streamer_guilds (twitch_user_id, guild_id, active, source, deactivated_at)
         SELECT p.twitch_user_id, $1,
                (p.status = 'active'
                 AND COALESCE(p.manual_partner_opt_out, 0) = 0
                 AND COALESCE(p.technical_pause_reason, '') = ''
                 AND p.admin_archived_at IS NULL),
                'migration',
                CASE WHEN p.status = 'active'
                     AND COALESCE(p.manual_partner_opt_out, 0) = 0
                     AND COALESCE(p.technical_pause_reason, '') = ''
                     AND p.admin_archived_at IS NULL THEN NULL ELSE NOW() END
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
    let metadata: (Option<String>, bool) = sqlx::query_as(
        "SELECT deactivated_by, deactivated_at IS NOT NULL FROM streamer_guilds WHERE twitch_user_id = '55'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(metadata, (Some("onboarding".to_string()), true));
    assert!(!active);
}

#[tokio::test]
async fn promote_ohne_verifizierte_guild_bleibt_noop() {
    let pool = pool_or_skip!("gm_promote");
    let mut tx = pool.begin().await.unwrap();
    let outcome = promote_streamer_to_partner(&mut tx, &args("neu", "77"), chrono::Utc::now())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(!outcome.reactivated);
    assert_eq!(
        outcome.hard_pause_reason.as_deref(),
        Some("guild_verification_required")
    );
    assert_eq!(outcome.guild_active, None);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partners")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM streamer_guilds")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
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

#[tokio::test]
async fn onboarding_bewahrt_admin_deaktivierung_und_aktive_bestandszeile() {
    let pool = pool_or_skip!("gm_admin_disabled");
    sqlx::query("INSERT INTO discord_guilds (guild_id, auto_partner_default) VALUES (88, TRUE)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO streamer_guilds (twitch_user_id, guild_id, active, source, deactivated_by, deactivated_at) VALUES ('88', 88, FALSE, 'onboarding', 'discord:admin', '2026-10-01T00:00:00Z')")
        .execute(&pool).await.unwrap();
    let mut chosen = args("disabled", "88");
    chosen.guild_id = Some(88);
    let mut tx = pool.begin().await.unwrap();
    let outcome = promote_streamer_to_partner(&mut tx, &chosen, chrono::Utc::now())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(!outcome.reactivated);
    assert_eq!(outcome.guild_active, Some(false));
    let row: (bool, String, String) = sqlx::query_as("SELECT active, deactivated_by, deactivated_at::text FROM streamer_guilds WHERE twitch_user_id = '88'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(
        row,
        (
            false,
            "discord:admin".to_string(),
            "2026-10-01 00:00:00+00".to_string()
        )
    );
    let mut tx = pool.begin().await.unwrap();
    attach_streamer_guild(&mut tx, "88", 88, true, "admin")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    sqlx::query("UPDATE discord_guilds SET auto_partner_default = FALSE WHERE guild_id = 88")
        .execute(&pool)
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    attach_streamer_guild(&mut tx, "88", 88, false, "onboarding")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let row: (bool, Option<String>, bool) = sqlx::query_as("SELECT active, deactivated_by, deactivated_at IS NULL FROM streamer_guilds WHERE twitch_user_id = '88'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(row, (true, None, true));
}

#[tokio::test]
async fn promotion_mit_auto_partner_aus_liefert_persistierte_inaktivitaet() {
    let pool = pool_or_skip!("gm_pending_outcome");
    sqlx::query("INSERT INTO discord_guilds (guild_id, auto_partner_default) VALUES (88, FALSE)")
        .execute(&pool)
        .await
        .unwrap();
    let mut chosen = args("pending", "88");
    chosen.guild_id = Some(88);
    let mut tx = pool.begin().await.unwrap();
    let outcome = promote_streamer_to_partner(&mut tx, &chosen, chrono::Utc::now())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(!outcome.reactivated);
    assert_eq!(outcome.guild_active, Some(false));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partners")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let timestamp: bool = sqlx::query_scalar(
        "SELECT deactivated_at IS NOT NULL FROM streamer_guilds WHERE twitch_user_id = '88'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(timestamp);
}

#[tokio::test]
async fn entfernte_community_wird_durch_aufnahme_nicht_installiert() {
    let pool = pool_or_skip!("gm_removed_community");
    sqlx::query(
        "INSERT INTO discord_guilds (guild_id, status, removed_at) VALUES ($1, 'active', NOW())",
    )
    .bind(COMMUNITY_GUILD_ID)
    .execute(&pool)
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    attach_streamer_guild(&mut tx, "88", COMMUNITY_GUILD_ID, true, "onboarding")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM streamer_guilds")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let removed: bool =
        sqlx::query_scalar("SELECT removed_at IS NOT NULL FROM discord_guilds WHERE guild_id = $1")
            .bind(COMMUNITY_GUILD_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(removed);
}

#[tokio::test]
async fn reauth_fremder_partner_erzeugt_keine_community_mitgliedschaft() {
    let pool = pool_or_skip!("gm_foreign_reauth");
    sqlx::query("INSERT INTO discord_guilds (guild_id) VALUES (88)")
        .execute(&pool)
        .await
        .unwrap();
    let mut chosen = args("foreign", "88");
    chosen.guild_id = Some(88);
    let mut tx = pool.begin().await.unwrap();
    promote_streamer_to_partner(&mut tx, &chosen, chrono::Utc::now())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    let outcome = promote_streamer_to_partner(&mut tx, &args("foreign", "88"), chrono::Utc::now())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(outcome.reactivated);
    let guilds: Vec<i64> =
        sqlx::query_scalar("SELECT guild_id FROM streamer_guilds WHERE twitch_user_id = '88'")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(guilds, vec![88]);
}

#[tokio::test]
async fn pending_freigabe_promotet_bestehende_identitaet_ohne_zweiten_store() {
    let pool = pool_or_skip!("gm_pending_approval_v2");
    sqlx::query("INSERT INTO discord_guilds (guild_id, auto_partner_default) VALUES (88, FALSE)")
        .execute(&pool)
        .await
        .unwrap();
    let mut chosen = args("pending_admitted", "8801");
    chosen.guild_id = Some(88);
    chosen.discord_user_id = Some("9901".to_string());
    let mut tx = pool.begin().await.unwrap();
    let pending = promote_streamer_to_partner(&mut tx, &chosen, chrono::Utc::now())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(!pending.reactivated);
    assert_eq!(pending.guild_active, Some(false));
    let saved: String = sqlx::query_scalar(
        "SELECT twitch_login FROM twitch_streamer_identities WHERE twitch_user_id = '8801'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(saved, "pending_admitted");
    let mut tx = pool.begin().await.unwrap();
    attach_streamer_guild(&mut tx, "8801", 88, true, "admin")
        .await
        .unwrap();
    let admitted =
        tb_raid::partner_setup::promote_admitted_streamer(&mut tx, "8801", chrono::Utc::now())
            .await
            .unwrap();
    assert!(admitted.reactivated);
    tx.commit().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    let repeated =
        tb_raid::partner_setup::promote_admitted_streamer(&mut tx, "8801", chrono::Utc::now())
            .await
            .unwrap();
    assert!(repeated.reactivated);
    tx.commit().await.unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partners WHERE twitch_user_id = '8801'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
    sqlx::query("UPDATE twitch_partners SET manual_partner_opt_out = 1, technical_pause_reason = 'token_error', admin_archived_at = '2026-01-01T00:00:00Z' WHERE twitch_user_id = '8801'")
        .execute(&pool).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    let preserved =
        tb_raid::partner_setup::promote_admitted_streamer(&mut tx, "8801", chrono::Utc::now())
            .await
            .unwrap();
    assert!(!preserved.reactivated);
    tx.commit().await.unwrap();
    let configuration: (i32, String, String) = sqlx::query_as("SELECT manual_partner_opt_out, technical_pause_reason, admin_archived_at FROM twitch_partners WHERE twitch_user_id = '8801'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(
        configuration,
        (
            1,
            "token_error".to_string(),
            "2026-01-01T00:00:00Z".to_string()
        )
    );
}

#[tokio::test]
async fn ausschluss_und_signup_block_verhindern_auch_inaktive_aufnahme() {
    let pool = pool_or_skip!("gm_guard_attachment");
    sqlx::query("INSERT INTO discord_guilds (guild_id) VALUES (88)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO twitch_exclusions (twitch_user_id, kind) VALUES ('11', 'banned')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO twitch_partner_signup_denylist (twitch_user_id, twitch_login, reason, added_by) VALUES ('12', 'denied', 'owner_decision', 'test')")
        .execute(&pool).await.unwrap();
    insert_partner(
        &pool,
        "13",
        "blocked",
        "active",
        0,
        Some("bot_banned"),
        false,
    )
    .await;
    let mut tx = pool.begin().await.unwrap();
    for uid in ["11", "12", "13"] {
        attach_streamer_guild(&mut tx, uid, 88, false, "onboarding")
            .await
            .unwrap();
    }
    let mut chosen = args("excluded", "11");
    chosen.guild_id = Some(88);
    let outcome = promote_streamer_to_partner(&mut tx, &chosen, chrono::Utc::now())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(!outcome.reactivated);
    assert_eq!(outcome.hard_pause_reason.as_deref(), Some("banned"));
    assert_eq!(outcome.guild_active, None);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM streamer_guilds")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

async fn legacy_fixture() -> test_postgres::TestPostgres {
    let db = test_postgres::TestPostgres::start().await;
    apply_ddl(&db.pool).await;
    sqlx::query("INSERT INTO discord_guilds (guild_id) VALUES ($1)")
        .bind(COMMUNITY_GUILD_ID)
        .execute(&db.pool)
        .await
        .unwrap();
    insert_partner(
        &db.pool,
        "8801",
        "legacy",
        "active",
        0,
        Some("token_error"),
        false,
    )
    .await;
    sqlx::query("INSERT INTO streamer_guilds (twitch_user_id, guild_id, active, source, deactivated_by, deactivated_at) VALUES ('8801', $1, FALSE, 'migration', 'migration', NOW())")
        .bind(COMMUNITY_GUILD_ID).execute(&db.pool).await.unwrap();
    db
}

#[tokio::test]
async fn legacy_technical_reauth_restores_only_established_german_admission() {
    let db = legacy_fixture().await;
    let pool = &db.pool;
    let mut tx = pool.begin().await.unwrap();
    let restored =
        promote_streamer_to_partner(&mut tx, &args("legacy", "8801"), chrono::Utc::now())
            .await
            .unwrap();
    assert!(restored.reactivated);
    tx.commit().await.unwrap();
    let row: (bool, String, Option<String>, bool) = sqlx::query_as("SELECT active, source, deactivated_by, deactivated_at IS NULL FROM streamer_guilds WHERE twitch_user_id = '8801'")
        .fetch_one(pool).await.unwrap();
    assert_eq!(row, (true, "migration".to_string(), None, true));
    let config: (i32, i32, Option<String>) = sqlx::query_as("SELECT manual_partner_opt_out, raid_bot_enabled, technical_pause_reason FROM twitch_partners WHERE twitch_user_id = '8801'")
        .fetch_one(pool).await.unwrap();
    assert_eq!(config, (0, 1, None));
    let mismatches: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partner_status_shadow_mismatch")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(mismatches, 0);
}

#[tokio::test]
async fn legacy_recovery_preserves_admin_deactivation_global_settings_and_bans() {
    let db = legacy_fixture().await;
    let pool = &db.pool;
    for change in [
        "UPDATE streamer_guilds SET deactivated_by = 'discord:admin'",
        "UPDATE streamer_guilds SET source = 'onboarding', deactivated_by = 'onboarding'",
        "UPDATE twitch_partners SET manual_partner_opt_out = 1",
        "UPDATE twitch_partners SET admin_archived_at = '2026-01-01T00:00:00Z'",
        "UPDATE twitch_partners SET status = 'archived'",
        "UPDATE twitch_partners SET technical_pause_reason = 'Bot_Banned'",
        "UPDATE discord_guilds SET removed_at = NOW()",
        "INSERT INTO twitch_exclusions VALUES ('8801', 'banned', NULL)",
        "INSERT INTO twitch_partner_signup_denylist (twitch_user_id, twitch_login, reason, added_by) VALUES ('8801', 'legacy', 'owner_decision', 'test')",
    ] {
        let mut tx = pool.begin().await.unwrap();
        sqlx::query(sqlx::AssertSqlSafe(change)).execute(&mut *tx).await.unwrap();
        let before: serde_json::Value = sqlx::query_scalar("SELECT to_jsonb(p) FROM twitch_partners p WHERE twitch_user_id = '8801'")
            .fetch_one(&mut *tx).await.unwrap();
        let outcome = promote_streamer_to_partner(&mut tx, &args("legacy", "8801"), chrono::Utc::now()).await.unwrap();
        assert!(!outcome.reactivated, "{change}");
        let active: bool = sqlx::query_scalar("SELECT active FROM streamer_guilds WHERE twitch_user_id = '8801'")
            .fetch_one(&mut *tx).await.unwrap();
        assert!(!active, "{change}");
        let after: serde_json::Value = sqlx::query_scalar("SELECT to_jsonb(p) FROM twitch_partners p WHERE twitch_user_id = '8801'")
            .fetch_one(&mut *tx).await.unwrap();
        assert_eq!(before, after, "{change}");
        tx.rollback().await.unwrap();
    }
    let mut tx = pool.begin().await.unwrap();
    let mut no_activation = args("legacy", "8801");
    no_activation.activate_partner_features = false;
    assert!(
        !promote_streamer_to_partner(&mut tx, &no_activation, chrono::Utc::now())
            .await
            .unwrap()
            .reactivated
    );
    tx.rollback().await.unwrap();
    sqlx::query("INSERT INTO discord_guilds (guild_id) VALUES (88)")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE streamer_guilds SET guild_id = 88")
        .execute(pool)
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    assert!(
        !promote_streamer_to_partner(&mut tx, &args("legacy", "8801"), chrono::Utc::now())
            .await
            .unwrap()
            .reactivated
    );
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn cutover_guard_rejects_membership_mismatch_before_view_switch() {
    let db = legacy_fixture().await;
    let pool = &db.pool;
    sqlx::query("UPDATE twitch_partners SET technical_pause_reason = NULL")
        .execute(pool)
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("CREATE OR REPLACE VIEW twitch_partners_all_state AS SELECT * FROM twitch_partners_legacy_state")
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    let cutover =
        include_str!("../../../migrations/20261013150300_multiserver_partner_cutover.sql")
            .replace("public.", "");
    assert!(sqlx::raw_sql(sqlx::AssertSqlSafe(cutover))
        .execute(&mut *tx)
        .await
        .is_err());
    tx.rollback().await.unwrap();
    let active: i32 = sqlx::query_scalar(
        "SELECT is_partner_active FROM twitch_partners_all_state WHERE twitch_user_id = '8801'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(active, 1);
    let mismatches: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partner_status_shadow_mismatch")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(mismatches, 1);
}

#[tokio::test]
async fn oauth_promotion_preserves_global_deactivation_with_active_admission() {
    let db = legacy_fixture().await;
    let pool = &db.pool;
    sqlx::query(
        "UPDATE streamer_guilds SET active = TRUE, deactivated_by = NULL, deactivated_at = NULL",
    )
    .execute(pool)
    .await
    .unwrap();
    for change in [
        "UPDATE twitch_partners SET manual_partner_opt_out = 1",
        "UPDATE twitch_partners SET admin_archived_at = '2026-01-01T00:00:00Z'",
        "UPDATE twitch_partners SET departnered_at = '2026-01-01T00:00:00Z'",
        "UPDATE twitch_partners SET status = 'archived'",
        "UPDATE twitch_partners SET technical_pause_reason = 'paused_by_admin'",
    ] {
        let mut tx = pool.begin().await.unwrap();
        sqlx::query(sqlx::AssertSqlSafe(change))
            .execute(&mut *tx)
            .await
            .unwrap();
        let before: serde_json::Value = sqlx::query_scalar(
            "SELECT to_jsonb(p) FROM twitch_partners p WHERE twitch_user_id = '8801'",
        )
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        let outcome =
            promote_streamer_to_partner(&mut tx, &args("legacy", "8801"), chrono::Utc::now())
                .await
                .unwrap();
        assert!(!outcome.reactivated, "{change}");
        let after: serde_json::Value = sqlx::query_scalar(
            "SELECT to_jsonb(p) FROM twitch_partners p WHERE twitch_user_id = '8801'",
        )
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(before, after, "{change}");
        tx.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn guild_approval_preserves_each_existing_global_configuration() {
    let db = legacy_fixture().await;
    let pool = &db.pool;
    sqlx::query("INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login) VALUES ('8801', 'legacy')")
        .execute(pool).await.unwrap();
    for change in [
        "UPDATE twitch_partners SET technical_pause_reason = NULL, manual_partner_opt_out = 1",
        "UPDATE twitch_partners SET technical_pause_reason = 'token_error'",
        "UPDATE twitch_partners SET admin_archived_at = '2026-01-01T00:00:00Z'",
        "UPDATE twitch_partners SET raid_bot_enabled = 0, technical_pause_reason = NULL",
    ] {
        let mut tx = pool.begin().await.unwrap();
        sqlx::query(sqlx::AssertSqlSafe(change))
            .execute(&mut *tx)
            .await
            .unwrap();
        let before: serde_json::Value = sqlx::query_scalar(
            "SELECT to_jsonb(p) FROM twitch_partners p WHERE twitch_user_id = '8801'",
        )
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        attach_streamer_guild(&mut tx, "8801", COMMUNITY_GUILD_ID, true, "admin")
            .await
            .unwrap();
        let outcome =
            tb_raid::partner_setup::promote_admitted_streamer(&mut tx, "8801", chrono::Utc::now())
                .await
                .unwrap();
        if change.contains("raid_bot_enabled") {
            assert!(outcome.reactivated);
        } else {
            assert!(!outcome.reactivated);
        }
        let after: serde_json::Value = sqlx::query_scalar(
            "SELECT to_jsonb(p) FROM twitch_partners p WHERE twitch_user_id = '8801'",
        )
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(before, after, "{change}");
        tx.rollback().await.unwrap();
    }
}
