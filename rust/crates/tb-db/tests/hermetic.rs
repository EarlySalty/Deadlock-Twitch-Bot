//! Hermetische tb-db-Tests gegen den Wegwerf-Container (`TB_TEST_DATABASE_URL`).
//! Ohne diese Env-Var werden die Tests laut übersprungen (kein stiller Pass).

use std::time::Duration;
use tb_config::DbConfig;
use tb_db::rows::{StreamerPlanRow, TwitchStreamerRow};

fn test_dsn() -> Option<String> {
    std::env::var("TB_TEST_DATABASE_URL").ok()
}

fn cfg(dsn: String) -> DbConfig {
    DbConfig {
        dsn,
        pool_max: 4,
        acquire_timeout: Duration::from_secs(5),
        connect_timeout: Duration::from_secs(5),
    }
}

macro_rules! skip_without_db {
    () => {
        match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!(
                    "SKIP: TB_TEST_DATABASE_URL nicht gesetzt — `rust/scripts/test_db.sh up`"
                );
                return;
            }
        }
    };
}

#[tokio::test]
async fn pool_connects_and_pings() {
    let dsn = skip_without_db!();
    let pool = tb_db::connect(&cfg(dsn)).await.expect("connect");
    let one: i32 = sqlx::query_scalar("SELECT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(one, 1);
}

/// Ersetzt den DB-Namen (letztes Pfadsegment) in der Test-DSN.
fn swap_db(dsn: &str, db: &str) -> String {
    let (base, _old) = dsn.rsplit_once('/').expect("DSN enthält '/'");
    format!("{base}/{db}")
}

fn cfg_single(dsn: String) -> DbConfig {
    DbConfig {
        pool_max: 1,
        ..cfg(dsn)
    }
}

/// F1-DoD: Eine frische, leere DB ist allein durch `run_migrations()` vollständig
/// aufsetzbar. Legt eine eigene Wegwerf-Datenbank an, installiert timescaledb,
/// migriert und prüft die erwarteten Schema-Objekte. Idempotenz wird durch das
/// zweimalige `run_migrations` mitgeprüft (zweiter Lauf = No-op gegen volles Schema).
#[tokio::test]
async fn run_migrations_builds_full_schema_on_fresh_db() {
    let admin_dsn = skip_without_db!();
    let admin = tb_db::connect(&cfg(admin_dsn.clone())).await.expect("admin connect");

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dbname = format!("tb_f1_{}_{}", std::process::id(), nanos);

    sqlx::query(&format!("DROP DATABASE IF EXISTS {dbname} WITH (FORCE)"))
        .execute(&admin)
        .await
        .ok();
    sqlx::query(&format!("CREATE DATABASE {dbname}"))
        .execute(&admin)
        .await
        .expect("create fresh db");

    let pool = tb_db::connect(&cfg_single(swap_db(&admin_dsn, &dbname)))
        .await
        .expect("connect fresh db");

    // CREATE EXTENSION timescaledb muss erster Befehl der Session sein.
    sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb")
        .execute(&pool)
        .await
        .expect("create timescaledb extension");

    // Migration zweimal -> Idempotenz gegen das frisch gebaute Vollschema.
    tb_db::run_migrations(&pool).await.expect("migrate (1st)");
    tb_db::run_migrations(&pool).await.expect("migrate (2nd, idempotent)");

    let scalar_i64 = |sql: &'static str| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, i64>(sql).fetch_one(&pool).await.unwrap() }
    };

    // ~60+ Tabellen (volles Storage-Schema + social_media + engagement + exp).
    let tables = scalar_i64(
        "SELECT count(*) FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_type = 'BASE TABLE'",
    )
    .await;
    assert!(tables >= 80, "erwartet >=80 Tabellen, gefunden {tables}");

    // Beide Views.
    let views = scalar_i64(
        "SELECT count(*) FROM information_schema.views WHERE table_schema = 'public' \
         AND table_name IN ('twitch_partners_all_state', 'twitch_streamers_partner_state')",
    )
    .await;
    assert_eq!(views, 2, "beide Partner-State-Views müssen existieren");

    // Identity-Sync-Trigger.
    let triggers = scalar_i64(
        "SELECT count(*) FROM pg_trigger WHERE NOT tgisinternal \
         AND tgname IN ('trg_twitch_streamers_sync_identity', 'trg_twitch_partners_sync_identity')",
    )
    .await;
    assert_eq!(triggers, 2, "beide Identity-Sync-Trigger müssen existieren");

    // Timescale-Hypertable + Compression.
    let hypertable = scalar_i64(
        "SELECT count(*) FROM timescaledb_information.hypertables \
         WHERE hypertable_name = 'twitch_observability_events' AND compression_enabled",
    )
    .await;
    assert_eq!(hypertable, 1, "observability-Hypertable mit Compression fehlt");

    // Raid-Identity-FKs (aus dem Repair-Pfad).
    let raid_fks = scalar_i64(
        "SELECT count(*) FROM pg_constraint WHERE contype = 'f' \
         AND conname LIKE '%raid_history_ref%'",
    )
    .await;
    assert_eq!(raid_fks, 2, "beide raid_history-Referenz-FKs müssen existieren");

    // Token-Lifecycle-Spalten.
    let token_cols = scalar_i64(
        "SELECT count(*) FROM information_schema.columns WHERE table_name = 'twitch_token_blacklist' \
         AND column_name IN ('grace_expires_at', 'user_dm_sent', 'reminder_sent', 'role_removed')",
    )
    .await;
    assert_eq!(token_cols, 4, "alle 4 Token-Lifecycle-Spalten müssen existieren");

    // social_media Phase 0: oauth consumed_at + Index.
    let consumed = scalar_i64(
        "SELECT count(*) FROM information_schema.columns \
         WHERE table_name = 'oauth_state_tokens' AND column_name = 'consumed_at'",
    )
    .await;
    assert_eq!(consumed, 1, "oauth_state_tokens.consumed_at fehlt");
    let consumed_idx = scalar_i64(
        "SELECT count(*) FROM pg_indexes WHERE indexname = 'idx_oauth_state_consumed_at'",
    )
    .await;
    assert_eq!(consumed_idx, 1, "Index idx_oauth_state_consumed_at fehlt");

    // exp_* und viewer_presence_ticks.
    let exp = scalar_i64(
        "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public' \
         AND table_name IN ('exp_sessions', 'exp_snapshots', 'exp_game_transitions')",
    )
    .await;
    assert_eq!(exp, 3, "alle drei exp_*-Tabellen müssen existieren");
    let presence = scalar_i64(
        "SELECT count(*) FROM information_schema.tables \
         WHERE table_name = 'twitch_viewer_presence_ticks'",
    )
    .await;
    assert_eq!(presence, 1, "twitch_viewer_presence_ticks fehlt");

    // Leaderboard-Indizes der Folgemigration (Ordering-Beweis: Baseline lief davor).
    let lb = scalar_i64(
        "SELECT count(*) FROM pg_indexes \
         WHERE indexname IN ('idx_twitch_stats_tracked_streamer_lower', \
                             'idx_twitch_stats_category_streamer_lower')",
    )
    .await;
    assert_eq!(lb, 2, "beide Leaderboard-Indizes müssen existieren");

    // M12-3: Auto-Approve-Settings-Seed (Python-Orakel `_ensure_auto_approve_settings`).
    // Drei Keys mit JSONB-`false`, damit eine frische DB die gleiche Schema-Parität
    // wie das gewachsene Prod-Schema hat — get_auto_approve_settings() liefert sonst
    // erst nach erstem Dashboard-PUT konsistente Zeilen.
    let auto_approve = scalar_i64(
        "SELECT count(*) FROM social_media_settings \
         WHERE key IN ('auto_approve_youtube', 'auto_approve_tiktok', 'auto_approve_instagram') \
           AND value = 'false'::jsonb",
    )
    .await;
    assert_eq!(
        auto_approve, 3,
        "alle drei Auto-Approve-Settings-Keys müssen mit value=false geseedet sein"
    );

    pool.close().await;
    sqlx::query(&format!("DROP DATABASE IF EXISTS {dbname} WITH (FORCE)"))
        .execute(&admin)
        .await
        .ok();
}

#[tokio::test]
async fn row_structs_map_real_columns() {
    let dsn = skip_without_db!();
    let pool = tb_db::connect(&cfg(dsn)).await.expect("connect");

    // Kontrolliertes DDL, das das Prod-Schema nachbildet (Timestamps als text!).
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS twitch_streamers (
            twitch_login TEXT PRIMARY KEY,
            twitch_user_id TEXT,
            discord_user_id TEXT,
            is_on_discord INTEGER DEFAULT 0,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            is_monitored_only INTEGER DEFAULT 0
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS streamer_plans (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT,
            plan_name TEXT NOT NULL DEFAULT 'free',
            promo_disabled INTEGER NOT NULL DEFAULT 0,
            activated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            expires_at TEXT,
            trial_ever_granted INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query("INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('dragskope', '42') ON CONFLICT DO NOTHING")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO streamer_plans (twitch_user_id, plan_name) VALUES ('42', 'pro') ON CONFLICT DO NOTHING")
        .execute(&pool).await.unwrap();

    let s: TwitchStreamerRow =
        sqlx::query_as("SELECT twitch_login, twitch_user_id, discord_user_id, is_on_discord, created_at, is_monitored_only FROM twitch_streamers WHERE twitch_login = 'dragskope'")
            .fetch_one(&pool).await.unwrap();
    assert_eq!(s.twitch_login, "dragskope");
    assert_eq!(s.twitch_user_id.as_deref(), Some("42"));
    assert!(s.created_at.is_some()); // text-Timestamp, kein timestamptz

    let p: StreamerPlanRow =
        sqlx::query_as("SELECT twitch_user_id, twitch_login, plan_name, promo_disabled, activated_at, expires_at, trial_ever_granted FROM streamer_plans WHERE twitch_user_id = '42'")
            .fetch_one(&pool).await.unwrap();
    assert_eq!(p.plan_name, "pro");
    assert_eq!(p.promo_disabled, 0);
}
