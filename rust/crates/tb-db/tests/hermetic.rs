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

const B2_SESSION_ID_BIGINT_MIGRATION: &str =
    include_str!("../../../migrations/20260622140000_b2_session_id_bigint.sql");
const EXP_SNAPSHOT_CONFLICT_INDEX_MIGRATION: &str =
    include_str!("../../../migrations/20260718063000_exp_snapshot_conflict_index.sql");

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

/// B2: Die korrigierende Migration muss gegen ein gewachsenes Schema laufen,
/// in dem die fehlerhafte 12:00-Migration die Session-IDs auf INTEGER gesetzt
/// hat. Der Test isoliert genau diesen Vertrag statt die bekannte Fresh-Kette
/// mit historischen Trigger-Abhaengigkeiten mitzuziehen.
#[tokio::test]
async fn b2_session_id_bigint_migration_repairs_integer_columns() {
    let admin_dsn = skip_without_db!();
    let admin = tb_db::connect(&cfg(admin_dsn.clone()))
        .await
        .expect("admin connect");

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dbname = format!("tb_b2_{}_{}", std::process::id(), nanos);

    sqlx::query(&format!("DROP DATABASE IF EXISTS {dbname} WITH (FORCE)"))
        .execute(&admin)
        .await
        .ok();
    sqlx::query(&format!("CREATE DATABASE {dbname}"))
        .execute(&admin)
        .await
        .expect("create b2 db");

    let pool = tb_db::connect(&cfg_single(swap_db(&admin_dsn, &dbname)))
        .await
        .expect("connect b2 db");

    sqlx::query(
        "CREATE TABLE public.twitch_stream_sessions (
            id BIGSERIAL PRIMARY KEY
        )",
    )
    .execute(&pool)
    .await
    .expect("create twitch_stream_sessions");
    sqlx::query(
        "CREATE TABLE public.twitch_live_state (
            twitch_user_id TEXT PRIMARY KEY,
            active_session_id INTEGER
        )",
    )
    .execute(&pool)
    .await
    .expect("create twitch_live_state");
    sqlx::query(
        "CREATE TABLE public.twitch_session_chatters (
            session_id INTEGER NOT NULL,
            chatter_login TEXT NOT NULL,
            PRIMARY KEY (session_id, chatter_login)
        )",
    )
    .execute(&pool)
    .await
    .expect("create twitch_session_chatters");

    assert_eq!(
        column_type(&pool, "twitch_live_state", "active_session_id").await,
        "integer"
    );
    assert_eq!(
        column_type(&pool, "twitch_session_chatters", "session_id").await,
        "integer"
    );

    sqlx::raw_sql(B2_SESSION_ID_BIGINT_MIGRATION)
        .execute(&pool)
        .await
        .expect("apply b2 migration");
    sqlx::raw_sql(B2_SESSION_ID_BIGINT_MIGRATION)
        .execute(&pool)
        .await
        .expect("apply b2 migration idempotently");

    assert_eq!(
        column_type(&pool, "twitch_live_state", "active_session_id").await,
        "bigint"
    );
    assert_eq!(
        column_type(&pool, "twitch_session_chatters", "session_id").await,
        "bigint"
    );

    pool.close().await;
    sqlx::query(&format!("DROP DATABASE IF EXISTS {dbname} WITH (FORCE)"))
        .execute(&admin)
        .await
        .ok();
}

#[tokio::test]
async fn exp_snapshot_migration_repairs_missing_conflict_index() {
    let admin_dsn = skip_without_db!();
    let admin = tb_db::connect(&cfg(admin_dsn.clone()))
        .await
        .expect("admin connect");

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dbname = format!("tb_exp_snapshot_{}_{}", std::process::id(), nanos);

    sqlx::query(&format!("DROP DATABASE IF EXISTS {dbname} WITH (FORCE)"))
        .execute(&admin)
        .await
        .ok();
    sqlx::query(&format!("CREATE DATABASE {dbname}"))
        .execute(&admin)
        .await
        .expect("create exp snapshot db");

    let pool = tb_db::connect(&cfg_single(swap_db(&admin_dsn, &dbname)))
        .await
        .expect("connect exp snapshot db");
    sqlx::query(
        "CREATE TABLE public.exp_snapshots (
            id BIGSERIAL PRIMARY KEY,
            exp_session_id BIGINT NOT NULL,
            ts_utc TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await
    .expect("create exp_snapshots");

    sqlx::raw_sql(EXP_SNAPSHOT_CONFLICT_INDEX_MIGRATION)
        .execute(&pool)
        .await
        .expect("apply exp snapshot conflict-index migration");

    for _ in 0..2 {
        sqlx::query(
            "INSERT INTO public.exp_snapshots (exp_session_id, ts_utc)
             VALUES (42, '2026-07-18T04:30:00Z')
             ON CONFLICT (exp_session_id, ts_utc) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("conflict target must be backed by a unique index");
    }
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM public.exp_snapshots")
        .fetch_one(&pool)
        .await
        .expect("count exp snapshots");
    assert_eq!(rows, 1);

    pool.close().await;
    sqlx::query(&format!("DROP DATABASE IF EXISTS {dbname} WITH (FORCE)"))
        .execute(&admin)
        .await
        .ok();
}

#[tokio::test]
async fn exp_snapshot_migration_preserves_existing_duplicates() {
    let admin_dsn = skip_without_db!();
    let admin = tb_db::connect(&cfg(admin_dsn.clone()))
        .await
        .expect("admin connect");

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dbname = format!("tb_exp_duplicates_{}_{}", std::process::id(), nanos);

    sqlx::query(&format!("DROP DATABASE IF EXISTS {dbname} WITH (FORCE)"))
        .execute(&admin)
        .await
        .ok();
    sqlx::query(&format!("CREATE DATABASE {dbname}"))
        .execute(&admin)
        .await
        .expect("create exp duplicates db");

    let pool = tb_db::connect(&cfg_single(swap_db(&admin_dsn, &dbname)))
        .await
        .expect("connect exp duplicates db");
    sqlx::query(
        "CREATE TABLE public.exp_snapshots (
            id BIGSERIAL PRIMARY KEY,
            exp_session_id BIGINT NOT NULL,
            ts_utc TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await
    .expect("create exp_snapshots");
    sqlx::query(
        "INSERT INTO public.exp_snapshots (exp_session_id, ts_utc)
         VALUES (42, '2026-07-18T04:30:00Z'), (42, '2026-07-18T04:30:00Z')",
    )
    .execute(&pool)
    .await
    .expect("seed duplicate snapshots");

    sqlx::raw_sql(EXP_SNAPSHOT_CONFLICT_INDEX_MIGRATION)
        .execute(&pool)
        .await
        .expect("migration must not fail or delete existing rows");

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM public.exp_snapshots")
        .fetch_one(&pool)
        .await
        .expect("count preserved duplicates");
    assert_eq!(rows, 2);

    pool.close().await;
    sqlx::query(&format!("DROP DATABASE IF EXISTS {dbname} WITH (FORCE)"))
        .execute(&admin)
        .await
        .ok();
}

/// F1-DoD: Eine frische, leere DB ist allein durch `run_migrations()` vollständig
/// aufsetzbar. Legt eine eigene Wegwerf-Datenbank an, installiert timescaledb,
/// migriert und prüft die erwarteten Schema-Objekte. Idempotenz wird durch das
/// zweimalige `run_migrations` mitgeprüft (zweiter Lauf = No-op gegen volles Schema).
#[tokio::test]
async fn run_migrations_builds_full_schema_on_fresh_db() {
    let admin_dsn = skip_without_db!();
    let admin = tb_db::connect(&cfg(admin_dsn.clone()))
        .await
        .expect("admin connect");

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
    tb_db::run_migrations(&pool)
        .await
        .expect("migrate (2nd, idempotent)");

    let scalar_i64 = |sql: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>(sql)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
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

    sqlx::query(
        r#"
        INSERT INTO twitch_partners
            (twitch_user_id, twitch_login, status, manual_partner_opt_out,
             raid_bot_enabled, admin_archived_at, technical_pause_reason,
             inactivity_flagged_at)
        VALUES
            ('1001', 'raid_off', 'active', 0, 0, NULL, NULL, NULL),
            ('1002', 'admin_archived', 'active', 0, 1, '2026-06-01T00:00:00Z', NULL, NULL),
            ('1003', 'inactive_flag', 'active', 0, 1, NULL, NULL, '2026-06-01T00:00:00Z'),
            ('1004', 'paused', 'active', 0, 1, NULL, 'maintenance', NULL),
            ('1005', 'opted_out', 'active', 1, 1, NULL, NULL, NULL)
        "#,
    )
    .execute(&pool)
    .await
    .expect("partner-state probe rows");

    let partner_state = |login: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_as::<_, (i32, String)>(
                "SELECT is_partner_active, operational_state \
                   FROM twitch_partners_all_state \
                  WHERE twitch_login = $1",
            )
            .bind(login)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    assert_eq!(
        partner_state("raid_off").await,
        (1, "active".to_string()),
        "raid_bot_enabled=0 darf Lifecycle-Tracking nicht deaktivieren"
    );
    assert_eq!(
        partner_state("admin_archived").await,
        (0, "inactive".to_string()),
        "admin_archived_at muss deaktivieren und darf nicht active anzeigen"
    );
    assert_eq!(
        partner_state("inactive_flag").await,
        (1, "inactive".to_string()),
        "Inaktivitaetsflag ist nur Anzeigezustand, nicht is_partner_active"
    );
    assert_eq!(
        partner_state("paused").await,
        (0, "maintenance".to_string()),
        "jede technische Pause deaktiviert"
    );
    assert_eq!(
        partner_state("opted_out").await,
        (0, "admin_non_partner".to_string()),
        "manueller Opt-out deaktiviert"
    );

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
    assert_eq!(
        hypertable, 1,
        "observability-Hypertable mit Compression fehlt"
    );

    // Raid-Identity-FKs (aus dem Repair-Pfad).
    let raid_fks = scalar_i64(
        "SELECT count(*) FROM pg_constraint WHERE contype = 'f' \
         AND conname LIKE '%raid_history_ref%'",
    )
    .await;
    assert_eq!(
        raid_fks, 2,
        "beide raid_history-Referenz-FKs müssen existieren"
    );

    // Token-Lifecycle-Spalten.
    let token_cols = scalar_i64(
        "SELECT count(*) FROM information_schema.columns WHERE table_name = 'twitch_token_blacklist' \
         AND column_name IN ('grace_expires_at', 'user_dm_sent', 'reminder_sent', 'role_removed')",
    )
    .await;
    assert_eq!(
        token_cols, 4,
        "alle 4 Token-Lifecycle-Spalten müssen existieren"
    );

    // WS-B: Session-Flags sind im kanonischen Prod-Schema BOOLEAN. Der
    // gleichnamige Live-State-Aggregatwert bleibt dagegen INTEGER 0/1.
    let session_flag_types: Vec<(String, String)> = sqlx::query_as(
        "SELECT column_name, data_type
           FROM information_schema.columns
          WHERE table_schema = 'public'
            AND table_name = 'twitch_stream_sessions'
            AND column_name IN ('is_mature', 'had_deadlock_in_session')
          ORDER BY column_name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        session_flag_types,
        vec![
            ("had_deadlock_in_session".to_string(), "boolean".to_string()),
            ("is_mature".to_string(), "boolean".to_string()),
        ],
        "twitch_stream_sessions Flags muessen BOOLEAN sein"
    );

    let live_deadlock_type: String = sqlx::query_scalar(
        "SELECT data_type
           FROM information_schema.columns
          WHERE table_schema = 'public'
            AND table_name = 'twitch_live_state'
            AND column_name = 'had_deadlock_in_session'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        live_deadlock_type, "integer",
        "twitch_live_state.had_deadlock_in_session bleibt INTEGER"
    );

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

    // Conversation-Scam-Guard: Settings-Defaults und vollständiges Audit-Schema.
    let scam_guard_tables = scalar_i64(
        "SELECT count(*) FROM information_schema.tables \
         WHERE table_schema = 'public' \
           AND table_name IN ('twitch_scam_guard_settings', 'twitch_scam_guard_verdicts')",
    )
    .await;
    assert_eq!(
        scam_guard_tables, 2,
        "beide Conversation-Scam-Guard-Tabellen müssen existieren"
    );

    sqlx::query("INSERT INTO twitch_scam_guard_settings (channel_login) VALUES ('fresh_channel')")
        .execute(&pool)
        .await
        .expect("insert scam guard settings defaults");
    let settings_defaults: (bool, String, f64, f64) = sqlx::query_as(
        "SELECT enabled, mode, threshold::float8, suggestion_floor::float8 \
         FROM twitch_scam_guard_settings WHERE channel_login = 'fresh_channel'",
    )
    .fetch_one(&pool)
    .await
    .expect("load scam guard settings defaults");
    assert!(settings_defaults.0);
    assert_eq!(settings_defaults.1, "auto_ban");
    assert!((settings_defaults.2 - 0.90).abs() < 0.000_001);
    assert!((settings_defaults.3 - 0.70).abs() < 0.000_001);

    let verdict_columns = scalar_i64(
        "SELECT count(*) FROM information_schema.columns \
         WHERE table_name = 'twitch_scam_guard_verdicts' \
           AND column_name IN ( \
             'id', 'channel_login', 'chatter_login', 'chatter_id', 'verdict', \
             'confidence', 'category', 'reasoning', 'transcript_snapshot', \
             'action_taken', 'created_at' \
           )",
    )
    .await;
    assert_eq!(
        verdict_columns, 11,
        "Conversation-Scam-Guard-Verdict-Schema ist unvollständig"
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

    // Kontrolliertes DDL, das das Prod-Schema nachbildet.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS twitch_streamers (
            twitch_login TEXT PRIMARY KEY,
            twitch_user_id TEXT,
            created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
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
        sqlx::query_as("SELECT twitch_login, twitch_user_id, created_at FROM twitch_streamers WHERE twitch_login = 'dragskope'")
            .fetch_one(&pool).await.unwrap();
    assert_eq!(s.twitch_login, "dragskope");
    assert_eq!(s.twitch_user_id.as_deref(), Some("42"));
    assert!(s.created_at.is_some());

    let p: StreamerPlanRow =
        sqlx::query_as("SELECT twitch_user_id, twitch_login, plan_name, promo_disabled, activated_at, expires_at, trial_ever_granted FROM streamer_plans WHERE twitch_user_id = '42'")
            .fetch_one(&pool).await.unwrap();
    assert_eq!(p.plan_name, "pro");
    assert_eq!(p.promo_disabled, 0);
}
