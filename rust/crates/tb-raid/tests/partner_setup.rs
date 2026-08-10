//! Hermetische DB-Tests für das Partner-Setup nach OAuth-Auth
//! (`promote_streamer_to_partner` + `PartnerSetupService`).
//!
//! DDL prod-treu (Schema-Dump 11.6.): `twitch_partners`-Timestamps TEXT,
//! Flags INTEGER, `live_ping_role_id` BIGINT; `twitch_streamer_identities`
//! created_at/updated_at TEXT; `twitch_streamers` nur noch Identitäts-Spalten.

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

macro_rules! pool_or_skip {
    ($schema:expr) => {{
        let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
            if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
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
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
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
        // Prod: alle Timestamp-Spalten TEXT, Flags INTEGER, live_ping_role_id BIGINT.
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
        r#"CREATE TABLE twitch_streamers (
            id BIGSERIAL PRIMARY KEY,
            twitch_login TEXT UNIQUE NOT NULL,
            twitch_user_id TEXT,
            created_at TIMESTAMPTZ DEFAULT NOW()
        )"#,
        r#"CREATE TABLE streamer_plans (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT,
            first_login_at TEXT,
            trials_granted INTEGER NOT NULL DEFAULT 0
        )"#,
        r#"CREATE TABLE twitch_raid_auth (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT
        )"#,
        // Signup-Denylist: eigenstaendiger Zustand "gehoert nicht ins
        // Partnerprogramm". Muss hier stehen, weil der Promote-Guard
        // fail-closed ist — eine fehlende Tabelle wuerde jede Promotion
        // abbrechen statt sie durchzulassen.
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
    ] {
        sqlx::query(ddl).execute(pool).await.unwrap();
    }
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
    }
}

async fn promote(pool: &PgPool, args: &PromotePartnerArgs) {
    let mut tx = pool.begin().await.unwrap();
    promote_streamer_to_partner(&mut tx, args, chrono::Utc::now())
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

// ---------------------------------------------------------------------------
// promote_streamer_to_partner
// ---------------------------------------------------------------------------

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
    // partnered_at = created_at der Quell-Zeile (::text-Rendering).
    assert!(partnered_at.is_some());

    // Identität geschrieben: expliziter Parameter gewinnt vor Quell-Zeile.
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

    // clear_source: Quelle gelöscht.
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
    // Der dokumentierte Bugfix: Python wipet silent_ban/silent_raid/
    // live_ping_role_id/require_discord_link bei Re-Promotion — Rust bewahrt.
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
    assert_eq!(row.7, None, "admin_archived_at = NULL (Re-Aktivierung)");
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
    // Python: COALESCE(EXCLUDED.x, bestehend) — None-Parameter überschreiben nicht.
    let pool = pool_or_skip!("ps_coalesce");
    sqlx::query(
        "INSERT INTO twitch_streamer_identities
            (twitch_user_id, twitch_login, discord_user_id, discord_display_name, is_on_discord)
         VALUES ('555', 'kanal', '777000', 'Bestand', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Re-Promotion ohne Discord-Parameter: sync-Flow liest die bestehende
    // Identität vorher und reicht sie durch — hier simulieren wir den
    // promote-Aufruf mit None (z. B. Identität nicht auflösbar).
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

// ---------------------------------------------------------------------------
// B11-PR-7: Hard-Pause-Guard (technical_pause_reason in {blocked, bot_banned})
// ---------------------------------------------------------------------------

/// Python `reactivate_partner_after_valid_auth` (`partner_registry.py:1366`):
/// Hard-Kills dürfen durch einen OAuth-Followup NICHT reaktiviert werden.
/// Der Followup ruft hier `promote_streamer_to_partner` — ohne den Guard würde
/// `technical_pause_reason = NULL` (Z.581) den Bann bedingungslos aufheben.
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

    // Pause-Grund + Deaktivierung bleiben unangetastet.
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
    // token_error o.ä. sind KEINE Hard-Kills → Promotion läuft, Pause wird aufgehoben.
    let pool = pool_or_skip!("ps_softpause");
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status,
            manual_partner_opt_out, raid_bot_enabled, technical_pause_reason)
         VALUES ('1003', 'tokenweg', 'active', 1, 0, 'token_error')",
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
async fn inaktive_admin_archivierung_blockiert_reauth_nicht() {
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
        result.reactivated,
        "admin_archived_at darf Reauth nicht blocken"
    );
    assert_eq!(result.hard_pause_reason, None);

    let (status, opt_out, raid, pause): (String, i32, i32, Option<String>) = sqlx::query_as(
        "SELECT status, manual_partner_opt_out, raid_bot_enabled, technical_pause_reason
         FROM twitch_partners
         WHERE twitch_user_id = '1004' AND status = 'active'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "active");
    assert_eq!((opt_out, raid), (0, 1));
    assert_eq!(pause, None);
}

#[tokio::test]
async fn selfservice_disconnect_wird_per_reauth_reaktiviert() {
    // Selbst getrennter Streamer: status='departnered', manual_partner_opt_out=1,
    // technical_pause_reason LEER. Ein Re-Auth muss ihn zurückholen — und dabei
    // seine Zeile reaktivieren statt eine zweite anzulegen, sonst verliert er
    // seine gesamte Kanal-Konfiguration.
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
    assert!(result.reactivated, "Opt-out ist kein Hard-Kill");
    assert_eq!(result.hard_pause_reason, None);

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
    assert_eq!(status, "active");
    assert_eq!((opt_out, raid), (0, 1), "voll reaktiviert");
    assert_eq!(departnered, None, "Trenn-Zeitpunkt geräumt");

    // Konfiguration überlebt den Reconnect.
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
    // Hard-Kill auf einer inaktiven Zeile: der Fallback auf die inaktive Zeile
    // darf den bestehenden Guard nicht aushebeln.
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

// ---------------------------------------------------------------------------
// record_first_login
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// PartnerSetupService-Orchestrierung (Mock-Ports)
// ---------------------------------------------------------------------------

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

#[tokio::test]
async fn sync_loest_discord_auf_und_setzt_rolle() {
    let pool = pool_or_skip!("ps_sync");
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

    // Display-Name kam aus dem Resolve (keine bestehende Identität).
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

    // Partner + first_login geschrieben.
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
    // Partner-Sync + first_login liefen trotzdem (Python: früher Return NACH Schritt 1+2).
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

// ---------------------------------------------------------------------------
// B10: revoke_streamer_role-Port (ohne DB)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn revoke_streamer_role_ruft_port_und_loggt() {
    let recorder = Recorder::default();
    DiscordDirectoryPort::revoke_streamer_role(&recorder, "12345", "Partner deautorisiert").await;
    assert_eq!(
        recorder.calls(),
        vec!["revoke:12345:Partner deautorisiert".to_string()]
    );
}

/// Eine Implementierung, die `revoke_streamer_role` NICHT überschreibt, nutzt
/// den Default (No-op-Log) — kein Panic, kein Hard-Fail.
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
    // Darf nicht panicken; Default-Impl ist ein reines Debug-Log.
    DiscordDirectoryPort::revoke_streamer_role(&port, "999", "egal").await;
}

// ---------------------------------------------------------------------------
// Signup-Block
// ---------------------------------------------------------------------------

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

/// Beweisziel: ein geblockter Streamer bekommt keine Partner-Zeile — auch nicht
/// halb, auch nicht pausiert. Der Guard gibt No-op zurueck, kein Fehler.
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

    assert!(result.signup_block.is_some(), "Block muss durchgereicht werden");
    assert_eq!(result.hard_pause_reason.as_deref(), Some("signup_blocked"));
    assert!(!result.reactivated);
    assert_eq!(
        partner_zeilen(&pool, "173926844").await,
        0,
        "geblockter Streamer darf keine Partner-Zeile haben"
    );
}

/// Beweisziel: der Block haengt an der stabilen ID. Benennt sich der Streamer
/// um, greift er trotzdem — sonst waere er per Namenswechsel aushebelbar.
#[tokio::test]
async fn signup_block_greift_nach_umbenennung() {
    let pool = pool_or_skip!("ps_block_rename");
    block_setzen(&pool, "173926844", "temmiee985", None).await;

    // Gleiche ID, neuer Login.
    let args = default_args("temmiee_neu", "173926844");
    let mut tx = pool.begin().await.unwrap();
    let result = promote_streamer_to_partner(&mut tx, &args, chrono::Utc::now())
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert!(result.signup_block.is_some());
    assert_eq!(partner_zeilen(&pool, "173926844").await, 0);
}

/// Beweisziel: die Richtungsregel gilt nur in eine Richtung. Ein Streamer, der
/// aus einem anderen Grund auf der Raid-Blacklist steht (Bot-Ban), darf sich
/// weiterhin ganz normal ins Partnerprogramm melden.
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

/// Beweisziel: faellt der Nachschlag aus (Tabelle weg, DB-Fehler), bricht die
/// Promotion ab, statt den Block still zu uebergehen.
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

/// Beweisziel: der Absagetext aus der DB schlaegt den Standardtext durch bis in
/// das Objekt, das die Antwort baut.
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
    // Der interne Grund darf nie im user-sichtbaren Text auftauchen.
    assert!(!block.public_text().contains("owner_decision"));
}

/// Beweisziel: ohne eigenen Text kommt der Standard-Absagetext, wortgleich.
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
