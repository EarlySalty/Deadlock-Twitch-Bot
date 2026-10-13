#[path = "../../../test-support/postgres.rs"]
mod test_postgres;
#[path = "../../../test-support/schema_sql.rs"]
mod test_sql;

use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_raid::partner_setup::{
    promote_streamer_to_partner, record_first_login, ChatGreeterPort, DiscordDirectoryPort,
    ModeratorInstallPort, PartnerSetupError, PartnerSetupService, PromotePartnerArgs,
};

mod test_database {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-support/database.rs"
    ));
}

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

mod referral_test_support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-support/streamer_referrals.rs"
    ));
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
            raid_admin_enabled BOOLEAN NOT NULL DEFAULT TRUE, raid_bot_enabled INTEGER,
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
        r#"CREATE UNIQUE INDEX idx_denylist_login
            ON twitch_partner_signup_denylist (lower(twitch_login))"#,
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
            streamer TEXT NOT NULL,
            viewer_count INTEGER,
            is_partner BOOLEAN DEFAULT FALSE,
            game_name TEXT,
            stream_title TEXT,
            tags TEXT
        )"#,
        r#"CREATE TABLE twitch_stats_tracked (
            ts_utc TIMESTAMPTZ NOT NULL,
            streamer TEXT NOT NULL,
            viewer_count INTEGER,
            is_partner BOOLEAN DEFAULT FALSE,
            game_name TEXT,
            stream_title TEXT,
            tags TEXT
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
        r#"INSERT INTO discord_guilds (guild_id, name, language)
            VALUES (1289721245281292288, 'Deutsche Deadlock Community', 'de')"#,
    ] {
        sqlx::query(ddl).execute(pool).await.unwrap();
    }
    referral_test_support::schema(pool).await;
}

fn default_args(login: &str, uid: &str) -> PromotePartnerArgs {
    PromotePartnerArgs {
        twitch_login: login.to_string(),
        twitch_user_id: uid.to_string(),
        discord_user_id: None,
        discord_display_name: None,
        is_on_discord: 0,
        activate_partner_features: true,
        clear_source: true,
        guild_id: Some(tb_raid::partner_setup::COMMUNITY_GUILD_ID),
    }
}

async fn promote(pool: &PgPool, args: &PromotePartnerArgs) {
    let mut tx = pool.begin().await.unwrap();
    promote_streamer_to_partner(&mut tx, args, chrono::Utc::now())
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn erst_promotion_legt_partner_an_und_loescht_quelle() {
    let pool = pool_or_skip!("ps_insert");
    sqlx::query(
        "INSERT INTO twitch_streamers (twitch_login, twitch_user_id)
         VALUES ('neuling', '111')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, discord_user_id, discord_display_name)
         VALUES ('111', 'neuling', '999000', 'Quell-Name')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let mut args = default_args("Neuling", "111");
    args.discord_user_id = Some("424242".to_string());
    args.discord_display_name = Some("Discord-Name".to_string());
    args.is_on_discord = 1;
    promote(&pool, &args).await;

    let (login, status, opt_out, raid, partnered_at): (String, String, i32, i32, Option<String>) =
        sqlx::query_as(
            "SELECT twitch_login, status, manual_partner_opt_out,
                raid_bot_enabled, partnered_at
         FROM twitch_partners WHERE twitch_user_id = '111'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(login, "neuling");
    assert_eq!(status, "active");
    assert_eq!((opt_out, raid), (0, 1));
    assert!(partnered_at.is_some());

    let (discord_id, display, on_discord): (Option<String>, Option<String>, Option<i32>) =
        sqlx::query_as(
            "SELECT discord_user_id, discord_display_name, is_on_discord
             FROM twitch_streamer_identities WHERE twitch_user_id = '111'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(discord_id.as_deref(), Some("424242"));
    assert_eq!(display.as_deref(), Some("Discord-Name"));
    assert_eq!(on_discord, Some(1));

    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM twitch_streamers WHERE twitch_user_id = '111'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining, 0);
}

#[tokio::test]
async fn re_promotion_bewahrt_partner_einstellungen() {
    let pool = pool_or_skip!("ps_update");
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, silent_ban,
            silent_raid, live_ping_role_id, live_ping_enabled, require_discord_link,
            added_by, last_description, partnered_at, admin_archived_at)
         VALUES ('222', 'veteran', 'active', 1, 1, 1313624729466441769, 0, 1,
            'admin', 'Beschreibung', '2025-01-01T00:00:00+00:00', '2025-06-01T00:00:00+00:00')",
    )
    .execute(&pool)
    .await
    .unwrap();

    promote(&pool, &default_args("veteran", "222")).await;

    type PreservedRow = (
        i32,
        i32,
        Option<i64>,
        i32,
        i32,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    );
    let row: PreservedRow = sqlx::query_as(
        "SELECT silent_ban, silent_raid, live_ping_role_id, live_ping_enabled,
                require_discord_link, added_by, partnered_at, admin_archived_at, status
         FROM twitch_partners WHERE twitch_user_id = '222'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, 1, "silent_ban bewahrt");
    assert_eq!(row.1, 1, "silent_raid bewahrt");
    assert_eq!(
        row.2,
        Some(1313624729466441769),
        "live_ping_role_id bewahrt"
    );
    assert_eq!(row.3, 0, "live_ping_enabled bewahrt");
    assert_eq!(row.4, 1, "require_discord_link bewahrt");
    assert_eq!(row.5.as_deref(), Some("admin"));
    assert_eq!(
        row.6.as_deref(),
        Some("2025-01-01T00:00:00+00:00"),
        "partnered_at bewahrt"
    );
    assert_eq!(row.7.as_deref(), Some("2025-06-01T00:00:00+00:00"));
    assert_eq!(row.8, "active");
}

#[tokio::test]
async fn identity_steal_entfernt_discord_id_von_anderem_streamer() {
    let pool = pool_or_skip!("ps_steal");
    sqlx::query(
        "INSERT INTO twitch_streamer_identities
            (twitch_user_id, twitch_login, discord_user_id, discord_display_name, is_on_discord)
         VALUES ('333', 'alter_kanal', '555000', 'Alt', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let mut args = default_args("neuer_kanal", "444");
    args.discord_user_id = Some("555000".to_string());
    args.is_on_discord = 1;
    promote(&pool, &args).await;

    let (old_discord, old_flag): (Option<String>, Option<i32>) = sqlx::query_as(
        "SELECT discord_user_id, is_on_discord FROM twitch_streamer_identities
         WHERE twitch_user_id = '333'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(old_discord, None, "Discord-ID vom alten Kanal entfernt");
    assert_eq!(old_flag, Some(0));

    let new_discord: Option<String> = sqlx::query_scalar(
        "SELECT discord_user_id FROM twitch_streamer_identities WHERE twitch_user_id = '444'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(new_discord.as_deref(), Some("555000"));
}

#[tokio::test]
async fn identity_upsert_coalesce_bewahrt_bestehende_werte() {
    let pool = pool_or_skip!("ps_coalesce");
    sqlx::query(
        "INSERT INTO twitch_streamer_identities
            (twitch_user_id, twitch_login, discord_user_id, discord_display_name, is_on_discord)
         VALUES ('555', 'kanal', '777000', 'Bestand', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    promote(&pool, &default_args("kanal", "555")).await;

    let (discord_id, display): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT discord_user_id, discord_display_name FROM twitch_streamer_identities
         WHERE twitch_user_id = '555'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(discord_id.as_deref(), Some("777000"), "COALESCE bewahrt");
    assert_eq!(display.as_deref(), Some("Bestand"));
}

#[tokio::test]
async fn related_tables_werden_normalisiert() {
    let pool = pool_or_skip!("ps_related");
    sqlx::query(
        "INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login) VALUES ('666', 'altname')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_live_state (twitch_user_id, streamer_login) VALUES ('666', 'altname')",
    )
    .execute(&pool)
    .await
    .unwrap();

    promote(&pool, &default_args("NeuName", "666")).await;

    let auth_login: String = sqlx::query_scalar(
        "SELECT twitch_login FROM twitch_raid_auth WHERE twitch_user_id = '666'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(auth_login, "neuname");
    let live_login: String = sqlx::query_scalar(
        "SELECT streamer_login FROM twitch_live_state WHERE twitch_user_id = '666'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(live_login, "neuname");
}

#[tokio::test]
async fn ungueltige_identitaet_wird_abgewiesen() {
    let pool = pool_or_skip!("ps_invalid");
    let mut tx = pool.begin().await.unwrap();
    let result =
        promote_streamer_to_partner(&mut tx, &default_args("", "777"), chrono::Utc::now()).await;
    assert!(result.is_err(), "leerer Login → InvalidIdentity");
}

#[tokio::test]
async fn hard_pause_blocked_wird_nicht_reaktiviert() {
    let pool = pool_or_skip!("ps_hardpause_blocked");
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status,
            manual_partner_opt_out, raid_bot_enabled, technical_pause_reason)
         VALUES ('1001', 'gebannt', 'active', 1, 0, 'blocked')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let result = {
        let mut tx = pool.begin().await.unwrap();
        let r = promote_streamer_to_partner(
            &mut tx,
            &default_args("gebannt", "1001"),
            chrono::Utc::now(),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        r
    };
    assert!(!result.reactivated, "Hard-Kill blockiert Reaktivierung");
    assert_eq!(result.hard_pause_reason.as_deref(), Some("blocked"));

    let (pause, opt_out, raid): (Option<String>, i32, i32) = sqlx::query_as(
        "SELECT technical_pause_reason, manual_partner_opt_out, raid_bot_enabled
         FROM twitch_partners WHERE twitch_user_id = '1001'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pause.as_deref(), Some("blocked"), "Pause-Grund bewahrt");
    assert_eq!((opt_out, raid), (1, 0), "Deaktivierung bewahrt");
}

#[tokio::test]
async fn hard_pause_bot_banned_case_insensitive() {
    let pool = pool_or_skip!("ps_hardpause_botbanned");
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status,
            manual_partner_opt_out, raid_bot_enabled, technical_pause_reason)
         VALUES ('1002', 'killed', 'active', 1, 0, '  Bot_Banned ')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let result = {
        let mut tx = pool.begin().await.unwrap();
        let r = promote_streamer_to_partner(
            &mut tx,
            &default_args("killed", "1002"),
            chrono::Utc::now(),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        r
    };
    assert!(
        !result.reactivated,
        "bot_banned (case/whitespace) blockiert"
    );
    assert_eq!(result.hard_pause_reason.as_deref(), Some("bot_banned"));

    let pause: Option<String> = sqlx::query_scalar(
        "SELECT technical_pause_reason FROM twitch_partners WHERE twitch_user_id = '1002'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        pause.as_deref(),
        Some("  Bot_Banned "),
        "Original-Grund unverändert"
    );
}

#[tokio::test]
async fn weiche_pause_wird_normal_reaktiviert() {
    let pool = pool_or_skip!("ps_softpause");
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status,
            manual_partner_opt_out, raid_bot_enabled, technical_pause_reason)
         VALUES ('1003', 'tokenweg', 'active', 0, 0, 'token_error')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let result = {
        let mut tx = pool.begin().await.unwrap();
        let r = promote_streamer_to_partner(
            &mut tx,
            &default_args("tokenweg", "1003"),
            chrono::Utc::now(),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        r
    };
    assert!(result.reactivated, "weiche Pause → Reaktivierung");
    assert_eq!(result.hard_pause_reason, None);

    let (pause, opt_out, raid): (Option<String>, i32, i32) = sqlx::query_as(
        "SELECT technical_pause_reason, manual_partner_opt_out, raid_bot_enabled
         FROM twitch_partners WHERE twitch_user_id = '1003'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pause, None, "Pause aufgehoben");
    assert_eq!((opt_out, raid), (0, 1), "voll reaktiviert");
}

#[tokio::test]
async fn inaktive_admin_archivierung_blockiert_reauth() {
    let pool = pool_or_skip!("ps_inactive_admin_archived");
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status,
            manual_partner_opt_out, raid_bot_enabled, admin_archived_at)
         VALUES ('1004', 'archiviert', 'archived', 1, 0, '2026-01-01T00:00:00+00:00')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let result = {
        let mut tx = pool.begin().await.unwrap();
        let r = promote_streamer_to_partner(
            &mut tx,
            &default_args("archiviert", "1004"),
            chrono::Utc::now(),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        r
    };
    assert!(
        !result.reactivated,
        "admin_archived_at muss erhalten bleiben"
    );
    assert_eq!(
        result.hard_pause_reason.as_deref(),
        Some("partner_configuration_inactive")
    );

    let (status, opt_out, raid, pause): (String, i32, i32, Option<String>) = sqlx::query_as(
        "SELECT status, manual_partner_opt_out, raid_bot_enabled, technical_pause_reason
         FROM twitch_partners
         WHERE twitch_user_id = '1004' AND status = 'archived'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "archived");
    assert_eq!((opt_out, raid), (1, 0));
    assert_eq!(pause, None);
}

#[tokio::test]
async fn selfservice_disconnect_bleibt_per_reauth_deaktiviert() {
    let pool = pool_or_skip!("ps_selfservice_disconnect");
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status,
            manual_partner_opt_out, raid_bot_enabled, departnered_at,
            require_discord_link, silent_ban, silent_raid, live_ping_role_id,
            live_ping_enabled, last_description, added_by, partnered_at)
         VALUES ('1005', 'getrennt', 'departnered', 1, 0, '2026-08-03T15:36:21+00:00',
                 1, 1, 1, 4242, 0, 'mein Kanal', 'admin:nani', '2025-10-10T13:05:44+00:00')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let result = {
        let mut tx = pool.begin().await.unwrap();
        let r = promote_streamer_to_partner(
            &mut tx,
            &default_args("getrennt", "1005"),
            chrono::Utc::now(),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        r
    };
    assert!(!result.reactivated);
    assert_eq!(
        result.hard_pause_reason.as_deref(),
        Some("partner_configuration_inactive")
    );

    let zeilen: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partners WHERE twitch_user_id = '1005'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(zeilen, 1, "keine zweite Zeile — die alte wird reaktiviert");

    let (status, opt_out, raid, departnered): (String, i32, i32, Option<String>) = sqlx::query_as(
        "SELECT status, manual_partner_opt_out, raid_bot_enabled, departnered_at
         FROM twitch_partners WHERE twitch_user_id = '1005'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "departnered");
    assert_eq!((opt_out, raid), (1, 0));
    assert_eq!(departnered.as_deref(), Some("2026-08-03T15:36:21+00:00"));

    let flags: (i32, i32, i32, Option<i64>, i32) = sqlx::query_as(
        "SELECT require_discord_link, silent_ban, silent_raid, live_ping_role_id,
                live_ping_enabled
         FROM twitch_partners WHERE twitch_user_id = '1005'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        flags,
        (1, 1, 1, Some(4242), 0),
        "Kanal-Konfiguration bleibt erhalten"
    );

    let texte: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT last_description, added_by, partnered_at
         FROM twitch_partners WHERE twitch_user_id = '1005'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        texte,
        (
            Some("mein Kanal".to_string()),
            Some("admin:nani".to_string()),
            Some("2025-10-10T13:05:44+00:00".to_string()),
        ),
        "Beschreibung, Herkunft und ursprünglicher Partner-Beginn bleiben stehen"
    );
}

#[tokio::test]
async fn departnerte_hard_kill_zeile_bleibt_unangetastet() {
    let pool = pool_or_skip!("ps_departnered_hardkill");
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status,
            manual_partner_opt_out, raid_bot_enabled, technical_pause_reason, silent_ban)
         VALUES ('1006', 'gebannt', 'departnered', 1, 0, 'bot_banned', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let result = {
        let mut tx = pool.begin().await.unwrap();
        let r = promote_streamer_to_partner(
            &mut tx,
            &default_args("gebannt", "1006"),
            chrono::Utc::now(),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        r
    };
    assert!(!result.reactivated, "bot_banned bleibt gesperrt");
    assert_eq!(result.hard_pause_reason.as_deref(), Some("bot_banned"));

    let (zeilen, status, opt_out, pause): (i64, String, i32, Option<String>) = sqlx::query_as(
        "SELECT COUNT(*) OVER (), status, manual_partner_opt_out, technical_pause_reason
         FROM twitch_partners WHERE twitch_user_id = '1006'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(zeilen, 1, "kein Insert trotz Re-Auth");
    assert_eq!(status, "departnered");
    assert_eq!(opt_out, 1);
    assert_eq!(pause.as_deref(), Some("bot_banned"));
}

#[tokio::test]
async fn first_login_coalesce_bewahrt_ersten_timestamp() {
    let pool = pool_or_skip!("ps_firstlogin");
    record_first_login(&pool, "888", "kanal").await;
    let first: Option<String> = sqlx::query_scalar(
        "SELECT first_login_at FROM streamer_plans WHERE twitch_user_id = '888'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(first.is_some());

    record_first_login(&pool, "888", "kanal").await;
    let second: Option<String> = sqlx::query_scalar(
        "SELECT first_login_at FROM streamer_plans WHERE twitch_user_id = '888'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(first, second, "COALESCE bewahrt den ersten Timestamp");
}

#[derive(Default)]
struct Recorder {
    calls: Mutex<Vec<String>>,
    resolve_result: Option<String>,
    greeter_ok: bool,
    moderator_error: Option<String>,
}

impl Recorder {
    fn log(&self, entry: impl Into<String>) {
        self.calls.lock().unwrap().push(entry.into());
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl DiscordDirectoryPort for Recorder {
    async fn resolve_display_name(&self, discord_user_id: &str) -> Option<String> {
        self.log(format!("resolve:{discord_user_id}"));
        self.resolve_result.clone()
    }
    async fn grant_streamer_role(&self, discord_user_id: &str, reason: &str) {
        self.log(format!("role:{discord_user_id}:{reason}"));
    }
    async fn revoke_streamer_role(&self, discord_user_id: &str, reason: &str) {
        self.log(format!("revoke:{discord_user_id}:{reason}"));
    }
}

#[async_trait]
impl ModeratorInstallPort for Recorder {
    async fn add_channel_moderator(
        &self,
        broadcaster_id: &str,
        bot_user_id: &str,
        _streamer_access_token: &str,
    ) -> Result<(), String> {
        self.log(format!("mod:{broadcaster_id}:{bot_user_id}"));
        match &self.moderator_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}

#[async_trait]
impl ChatGreeterPort for Recorder {
    async fn send_partner_chat_message(
        &self,
        twitch_login: &str,
        message: &str,
    ) -> Result<bool, String> {
        self.log(format!("chat:{twitch_login}:{message}"));
        Ok(self.greeter_ok)
    }
}

fn service(pool: PgPool, recorder: Arc<Recorder>, bot_id: Option<&str>) -> PartnerSetupService {
    PartnerSetupService::new(
        pool,
        recorder.clone(),
        recorder.clone(),
        recorder,
        bot_id.map(str::to_string),
    )
    .with_pauses(Duration::ZERO, Duration::ZERO)
}

async fn legacy_partner(pool: &PgPool, uid: &str, login: &str) {
    promote(pool, &default_args(login, uid)).await;
}

#[tokio::test]
async fn sync_loest_discord_auf_und_setzt_rolle() {
    let pool = pool_or_skip!("ps_sync");
    legacy_partner(&pool, "901", "synckanal").await;
    let recorder = Arc::new(Recorder {
        resolve_result: Some("Globaler Name".to_string()),
        greeter_ok: true,
        ..Default::default()
    });
    let svc = service(pool.clone(), recorder.clone(), Some("botid"));

    let result = svc
        .sync_partner_state_after_auth("901", "synckanal", Some("123456789"), true)
        .await
        .unwrap();
    assert_eq!(result.discord_user_id.as_deref(), Some("123456789"));
    assert!(result.signup_block.is_none());

    let display: Option<String> = sqlx::query_scalar(
        "SELECT discord_display_name FROM twitch_streamer_identities WHERE twitch_user_id = '901'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(display.as_deref(), Some("Globaler Name"));

    let calls = recorder.calls();
    assert!(calls.contains(&"resolve:123456789".to_string()));
    assert!(calls
        .iter()
        .any(|c| c.starts_with("role:123456789:Twitch-Bot erfolgreich autorisiert")));
}

#[tokio::test]
async fn sync_ohne_discord_id_setzt_keine_rolle() {
    let pool = pool_or_skip!("ps_sync_no_discord");
    legacy_partner(&pool, "902", "ohnediscord").await;
    let recorder = Arc::new(Recorder::default());
    let svc = service(pool.clone(), recorder.clone(), Some("botid"));

    let result = svc
        .sync_partner_state_after_auth("902", "ohnediscord", None, true)
        .await
        .unwrap();
    assert_eq!(result.discord_user_id, None);
    assert!(result.signup_block.is_none());
    assert!(recorder.calls().is_empty(), "kein Resolve, keine Rolle");

    let is_on_discord: Option<i32> = sqlx::query_scalar(
        "SELECT is_on_discord FROM twitch_streamer_identities WHERE twitch_user_id = '902'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(is_on_discord, Some(0));
}

#[tokio::test]
async fn complete_setup_voller_ablauf() {
    let pool = pool_or_skip!("ps_complete");
    legacy_partner(&pool, "903", "vollkanal").await;
    let recorder = Arc::new(Recorder {
        greeter_ok: true,
        ..Default::default()
    });
    let svc = service(pool.clone(), recorder.clone(), Some("botid"));

    svc.complete_setup_for_streamer("903", "vollkanal", "token-abc", None)
        .await
        .expect("vollständiges Setup");

    let calls = recorder.calls();
    assert!(
        calls.contains(&"mod:903:botid".to_string()),
        "Moderator-Schritt: {calls:?}"
    );
    let chat_calls: Vec<_> = calls.iter().filter(|c| c.starts_with("chat:")).collect();
    assert_eq!(chat_calls.len(), 3, "drei Begrüßungsnachrichten: {calls:?}");
    assert!(chat_calls[0].contains("Deadlock Chatbot Guard verbunden"));

    let status: String =
        sqlx::query_scalar("SELECT status FROM twitch_partners WHERE twitch_user_id = '903'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "active");
    let first: Option<String> = sqlx::query_scalar(
        "SELECT first_login_at FROM streamer_plans WHERE twitch_user_id = '903'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(first.is_some());
}

#[tokio::test]
async fn complete_setup_ohne_bot_id_ueberspringt_mod_und_chat() {
    let pool = pool_or_skip!("ps_no_botid");
    legacy_partner(&pool, "904", "kein_bot").await;
    let recorder = Arc::new(Recorder {
        greeter_ok: true,
        ..Default::default()
    });
    let svc = service(pool.clone(), recorder.clone(), None);

    let result = svc
        .complete_setup_for_streamer("904", "kein_bot", "token", None)
        .await;

    assert!(
        result.is_ok(),
        "erfolgreicher Partner-Sync bestimmt das Ergebnis"
    );

    let calls = recorder.calls();
    assert!(
        !calls
            .iter()
            .any(|c| c.starts_with("mod:") || c.starts_with("chat:")),
        "ohne Bot-ID weder Moderator noch Chat: {calls:?}"
    );
    let status: String =
        sqlx::query_scalar("SELECT status FROM twitch_partners WHERE twitch_user_id = '904'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "active");
}

#[tokio::test]
async fn complete_setup_bleibt_bei_moderator_fehler_erfolgreich() {
    let pool = pool_or_skip!("ps_mod_error");
    legacy_partner(&pool, "907", "mod_fehler").await;
    let recorder = Arc::new(Recorder {
        moderator_error: Some("keine Moderator-Rechte".to_string()),
        ..Default::default()
    });
    let svc = service(pool, recorder, Some("botid"));

    let result = svc
        .complete_setup_for_streamer("907", "mod_fehler", "token", None)
        .await;

    assert!(
        result.is_ok(),
        "erfolgreicher Partner-Sync bestimmt das Ergebnis"
    );
}

#[tokio::test]
async fn complete_setup_gibt_partner_sync_fehler_zurueck() {
    let pool = pool_or_skip!("ps_sync_error");
    let recorder = Arc::new(Recorder::default());
    let svc = service(pool, recorder, Some("botid"));

    let result = svc.complete_setup_for_streamer("", "", "token", None).await;

    assert!(matches!(result, Err(PartnerSetupError::InvalidIdentity)));
}

#[tokio::test]
async fn greeter_nicht_verfuegbar_bricht_restliche_nachrichten_ab() {
    let pool = pool_or_skip!("ps_greeter_off");
    legacy_partner(&pool, "905", "stiller_kanal").await;
    let recorder = Arc::new(Recorder {
        greeter_ok: false,
        ..Default::default()
    });
    let svc = service(pool.clone(), recorder.clone(), Some("botid"));

    svc.complete_setup_for_streamer("905", "stiller_kanal", "token", None)
        .await
        .expect("Moderator-Setup bleibt trotz fehlender Begrüßung erfolgreich");

    let chat_calls = recorder
        .calls()
        .iter()
        .filter(|c| c.starts_with("chat:"))
        .count();
    assert_eq!(chat_calls, 1, "erste Nachricht ok:false → Abbruch");
}

#[tokio::test]
async fn backfill_ist_idempotent() {
    let pool = pool_or_skip!("ps_backfill");
    legacy_partner(&pool, "906", "backkanal").await;
    sqlx::query(
        "INSERT INTO twitch_stats_category (ts_utc, streamer, viewer_count)
         VALUES ('2026-06-01T00:00:00Z', 'backkanal', 10), ('2026-06-01T00:01:00Z', 'backkanal', 12)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let recorder = Arc::new(Recorder::default());
    let svc = service(pool.clone(), recorder, Some("botid"));

    svc.sync_partner_state_after_auth("906", "backkanal", None, true)
        .await
        .unwrap();
    svc.sync_partner_state_after_auth("906", "backkanal", None, true)
        .await
        .unwrap();

    let tracked: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM twitch_stats_tracked WHERE streamer = 'backkanal'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tracked, 2, "NOT-EXISTS-Guard verhindert Duplikate");
}

#[tokio::test]
async fn revoke_streamer_role_ruft_port_und_loggt() {
    let recorder = Recorder::default();
    DiscordDirectoryPort::revoke_streamer_role(&recorder, "12345", "Partner deautorisiert").await;
    assert_eq!(
        recorder.calls(),
        vec!["revoke:12345:Partner deautorisiert".to_string()]
    );
}

struct GrantOnly;

#[async_trait]
impl DiscordDirectoryPort for GrantOnly {
    async fn resolve_display_name(&self, _discord_user_id: &str) -> Option<String> {
        None
    }
    async fn grant_streamer_role(&self, _discord_user_id: &str, _reason: &str) {}
}

#[tokio::test]
async fn revoke_streamer_role_default_ist_noop() {
    let port = GrantOnly;
    DiscordDirectoryPort::revoke_streamer_role(&port, "999", "egal").await;
}

async fn block_setzen(pool: &PgPool, uid: &str, login: &str, public_message: Option<&str>) {
    sqlx::query(
        "INSERT INTO twitch_partner_signup_denylist
             (twitch_user_id, twitch_login, reason, public_message, added_by)
         VALUES ($1, $2, 'owner_decision:repraesentation', $3, 'test')",
    )
    .bind(uid)
    .bind(login)
    .bind(public_message)
    .execute(pool)
    .await
    .unwrap();
}

async fn partner_zeilen(pool: &PgPool, uid: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partners WHERE twitch_user_id = $1")
        .bind(uid)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn signup_block_legt_keine_partner_zeile_an() {
    let pool = pool_or_skip!("ps_block_kein_insert");
    block_setzen(&pool, "173926844", "temmiee985", None).await;

    let args = default_args("temmiee985", "173926844");
    let mut tx = pool.begin().await.unwrap();
    let result = promote_streamer_to_partner(&mut tx, &args, chrono::Utc::now())
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert!(
        result.signup_block.is_some(),
        "Block muss durchgereicht werden"
    );
    assert_eq!(result.hard_pause_reason.as_deref(), Some("signup_blocked"));
    assert!(!result.reactivated);
    assert_eq!(
        partner_zeilen(&pool, "173926844").await,
        0,
        "geblockter Streamer darf keine Partner-Zeile haben"
    );
}

#[tokio::test]
async fn signup_block_greift_nach_umbenennung() {
    let pool = pool_or_skip!("ps_block_rename");
    block_setzen(&pool, "173926844", "temmiee985", None).await;

    let args = default_args("temmiee_neu", "173926844");
    let mut tx = pool.begin().await.unwrap();
    let result = promote_streamer_to_partner(&mut tx, &args, chrono::Utc::now())
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert!(result.signup_block.is_some());
    assert_eq!(partner_zeilen(&pool, "173926844").await, 0);
}

#[tokio::test]
async fn raid_blacklist_allein_blockt_signup_nicht() {
    let pool = pool_or_skip!("ps_block_nur_raid");
    sqlx::query(
        "INSERT INTO twitch_raid_blacklist (target_login, target_id, reason, added_at)
         VALUES ('botbanner', '444', 'bot_banned', '2026-01-01T00:00:00+00:00')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let args = default_args("botbanner", "444");
    let mut tx = pool.begin().await.unwrap();
    let result = promote_streamer_to_partner(&mut tx, &args, chrono::Utc::now())
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert!(result.signup_block.is_none());
    assert_eq!(
        partner_zeilen(&pool, "444").await,
        1,
        "Raid-Blacklist allein ist kein Signup-Block"
    );
}

#[tokio::test]
async fn signup_block_nachschlag_fehler_bricht_ab() {
    let pool = pool_or_skip!("ps_block_failclosed");
    sqlx::query("DROP TABLE twitch_partner_signup_denylist")
        .execute(&pool)
        .await
        .unwrap();

    let args = default_args("irgendwer", "555");
    let mut tx = pool.begin().await.unwrap();
    let err = promote_streamer_to_partner(&mut tx, &args, chrono::Utc::now())
        .await
        .unwrap_err();
    drop(tx);

    assert!(
        matches!(err, PartnerSetupError::SignupBlockLookupFailed),
        "erwartet SignupBlockLookupFailed, war {err:?}"
    );
    assert_eq!(partner_zeilen(&pool, "555").await, 0);
}

#[tokio::test]
async fn signup_block_liefert_eigenen_absagetext() {
    let pool = pool_or_skip!("ps_block_text");
    block_setzen(&pool, "839304219", "taiju_redestein", Some("Eigener Text.")).await;

    let args = default_args("taiju_redestein", "839304219");
    let mut tx = pool.begin().await.unwrap();
    let result = promote_streamer_to_partner(&mut tx, &args, chrono::Utc::now())
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let block = result.signup_block.expect("Block erwartet");
    assert_eq!(block.public_text(), "Eigener Text.");
    assert_eq!(block.public_body_html(), "<p>Eigener Text.</p>");
    assert!(!block.public_text().contains("owner_decision"));
}

#[tokio::test]
async fn signup_block_ohne_eigenen_text_nutzt_standard() {
    let pool = pool_or_skip!("ps_block_standardtext");
    block_setzen(&pool, "166907981", "ludi7", None).await;

    let args = default_args("ludi7", "166907981");
    let mut tx = pool.begin().await.unwrap();
    let result = promote_streamer_to_partner(&mut tx, &args, chrono::Utc::now())
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let block = result.signup_block.expect("Block erwartet");
    assert_eq!(block.public_text(), tb_domain::SIGNUP_BLOCK_BODY);
    assert!(block.public_text().contains("repräsentieren"));
}

#[tokio::test]
async fn promote_schreibt_aktive_streamer_guilds_zeile() {
    let pool = pool_or_skip!("ps_guild_write");
    promote(&pool, &default_args("guilded", "4242")).await;
    let row: (i64, bool, String) = sqlx::query_as(
        "SELECT guild_id, active, source FROM streamer_guilds WHERE twitch_user_id = '4242'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, tb_raid::partner_setup::COMMUNITY_GUILD_ID);
    assert!(row.1);
    assert_eq!(row.2, "onboarding");
}

#[tokio::test]
async fn migrated_technical_reauth_restores_service_followups() {
    let db = test_postgres::TestPostgres::start().await;
    apply_ddl(&db.pool).await;
    sqlx::raw_sql("INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, manual_partner_opt_out, raid_bot_enabled, technical_pause_reason) VALUES ('8801', 'legacy', 'active', 0, 0, 'token_error');
        INSERT INTO streamer_guilds (twitch_user_id, guild_id, active, source, deactivated_by, deactivated_at) VALUES ('8801', 1289721245281292288, FALSE, 'migration', 'migration', NOW());")
        .execute(&db.pool).await.unwrap();
    let recorder = Arc::new(Recorder::default());
    let svc = service(db.pool.clone(), recorder.clone(), None);
    let outcome = svc
        .sync_partner_state_after_auth("8801", "legacy", Some("9911"), true)
        .await
        .unwrap();
    assert!(outcome.reactivated);
    assert!(recorder
        .calls()
        .iter()
        .any(|call| call.starts_with("role:9911:")));
    let state: (bool, Option<String>) = sqlx::query_as("SELECT sg.active, p.technical_pause_reason FROM streamer_guilds sg JOIN twitch_partners p USING (twitch_user_id) WHERE sg.twitch_user_id = '8801'")
        .fetch_one(&db.pool).await.unwrap();
    assert_eq!(state, (true, None));
}

#[tokio::test]
async fn pending_membership_does_not_run_setup_side_effects() {
    let db = test_postgres::TestPostgres::start().await;
    apply_ddl(&db.pool).await;
    sqlx::raw_sql("INSERT INTO discord_guilds (guild_id, auto_partner_default) VALUES (88, FALSE);
        INSERT INTO streamer_guilds (twitch_user_id, guild_id, active, source, deactivated_by, deactivated_at) VALUES ('8801', 88, FALSE, 'onboarding', 'onboarding', NOW());")
        .execute(&db.pool).await.unwrap();
    let recorder = Arc::new(Recorder::default());
    let svc = service(db.pool.clone(), recorder.clone(), Some("botid"));
    svc.complete_setup_for_streamer("8801", "pending", "test-only", None)
        .await
        .unwrap();
    assert!(recorder.calls().is_empty());
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM twitch_partners), (SELECT COUNT(*) FROM streamer_plans)",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(counts, (0, 0));
}

include!("fixtures/streamer_referral_cases.rs");
