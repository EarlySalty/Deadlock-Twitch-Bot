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
    ModeratorInstallPort, PartnerSetupService, PromotePartnerArgs,
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
            manual_verified_permanent INTEGER,
            manual_verified_until TEXT,
            manual_verified_at TEXT,
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
            created_at TIMESTAMPTZ DEFAULT NOW(),
            discord_user_id TEXT,
            discord_display_name TEXT,
            is_on_discord INTEGER DEFAULT 0,
            archived_at TIMESTAMPTZ,
            is_monitored_only INTEGER DEFAULT 0
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
        manual_verified_at: "2026-06-12T10:00:00.000000+00:00".to_string(),
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
        "INSERT INTO twitch_streamers (twitch_login, twitch_user_id, discord_user_id, discord_display_name)
         VALUES ('neuling', '111', '999000', 'Quell-Name')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let mut args = default_args("Neuling", "111");
    args.discord_user_id = Some("424242".to_string());
    args.discord_display_name = Some("Discord-Name".to_string());
    args.is_on_discord = 1;
    promote(&pool, &args).await;

    let (login, status, mvp, opt_out, raid, partnered_at): (
        String,
        String,
        i32,
        i32,
        i32,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT twitch_login, status, manual_verified_permanent, manual_partner_opt_out,
                raid_bot_enabled, partnered_at
         FROM twitch_partners WHERE twitch_user_id = '111'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(login, "neuling");
    assert_eq!(status, "active");
    assert_eq!((mvp, opt_out, raid), (1, 0, 1));
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
    let remaining: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::bigint FROM twitch_streamers WHERE twitch_user_id = '111'")
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
    assert_eq!(row.2, Some(1313624729466441769), "live_ping_role_id bewahrt");
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
    sqlx::query("INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login) VALUES ('666', 'altname')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login) VALUES ('666', 'altname')")
        .execute(&pool)
        .await
        .unwrap();

    promote(&pool, &default_args("NeuName", "666")).await;

    let auth_login: String =
        sqlx::query_scalar("SELECT twitch_login FROM twitch_raid_auth WHERE twitch_user_id = '666'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(auth_login, "neuname");
    let live_login: String =
        sqlx::query_scalar("SELECT streamer_login FROM twitch_live_state WHERE twitch_user_id = '666'")
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
// record_first_login
// ---------------------------------------------------------------------------

#[tokio::test]
async fn first_login_coalesce_bewahrt_ersten_timestamp() {
    let pool = pool_or_skip!("ps_firstlogin");
    record_first_login(&pool, "888", "kanal").await;
    let first: Option<String> =
        sqlx::query_scalar("SELECT first_login_at FROM streamer_plans WHERE twitch_user_id = '888'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(first.is_some());

    record_first_login(&pool, "888", "kanal").await;
    let second: Option<String> =
        sqlx::query_scalar("SELECT first_login_at FROM streamer_plans WHERE twitch_user_id = '888'")
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
    ) {
        self.log(format!("mod:{broadcaster_id}:{bot_user_id}"));
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
    assert_eq!(result.as_deref(), Some("123456789"));

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
    assert_eq!(result, None);
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
        .await;

    let calls = recorder.calls();
    assert!(calls.contains(&"mod:903:botid".to_string()), "Moderator-Schritt: {calls:?}");
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
    let first: Option<String> =
        sqlx::query_scalar("SELECT first_login_at FROM streamer_plans WHERE twitch_user_id = '903'")
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

    svc.complete_setup_for_streamer("904", "kein_bot", "token", None)
        .await;

    let calls = recorder.calls();
    assert!(
        !calls.iter().any(|c| c.starts_with("mod:") || c.starts_with("chat:")),
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
async fn greeter_nicht_verfuegbar_bricht_restliche_nachrichten_ab() {
    let pool = pool_or_skip!("ps_greeter_off");
    let recorder = Arc::new(Recorder {
        greeter_ok: false,
        ..Default::default()
    });
    let svc = service(pool.clone(), recorder.clone(), Some("botid"));

    svc.complete_setup_for_streamer("905", "stiller_kanal", "token", None)
        .await;

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
