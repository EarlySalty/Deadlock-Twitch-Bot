//! Gemeinsame Test-Infrastruktur der hermetischen tb-monitoring-Tests:
//! Schema pro Test (Isolation bei parallelem Lauf) + prod-verifiziertes DDL
//! (Stand 2026-06-22): Session-Flags
//! (is_mature/had_deadlock_in_session) sind BOOLEAN wie Prod, Live-State bleibt
//! bei INTEGER-Flags, exp_* bei TEXT-Timestamps.

use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;

/// Frisches Schema + alle Monitoring-Tabellen. `None` = `TB_TEST_DATABASE_URL`
/// fehlt (laut überspringen, `rust/scripts/test_db.sh up`).
/// Wenn `TB_TEST_REQUIRE_DB=1` gesetzt ist, wird statt des stillen Skips
/// ein Panic ausgelöst — damit CI-Läufe mit DB keine grünen Phantoms liefern.
// Nicht jedes Test-Binary nutzt beide Fixture-Builder (geteiltes Modul, pro
// Binary kompiliert) → dead_code-Warnung ist erwartet/harmlos.
#[allow(dead_code)]
pub async fn pool_in_schema(schema: &str) -> Option<PgPool> {
    let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
        if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
            panic!(
                "TB_TEST_REQUIRE_DB=1 ist gesetzt, aber TB_TEST_DATABASE_URL fehlt — \
                 `rust/scripts/test_db.sh up` ausführen"
            );
        }
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt — `rust/scripts/test_db.sh up`");
        return None;
    };
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("admin connect");
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;

    let opts = PgConnectOptions::from_str(&dsn)
        .expect("dsn parse")
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .expect("connect");

    for ddl in [
        // Betriebstabellen, die der Rename per stabiler ID mitzieht.
        "CREATE TABLE twitch_engagement_log (
            id BIGSERIAL PRIMARY KEY, channel_login TEXT NOT NULL, channel_user_id TEXT,
            created_at TIMESTAMPTZ DEFAULT NOW())",
        "CREATE TABLE twitch_engagement_stream_transcripts (
            id BIGSERIAL PRIMARY KEY, channel_login TEXT NOT NULL, channel_user_id TEXT)",
        "CREATE TABLE twitch_outreach_shadow_events (
            id BIGSERIAL PRIMARY KEY, cycle_id TEXT UNIQUE, channel_login TEXT NOT NULL,
            channel_user_id TEXT)",
        "CREATE TABLE twitch_scam_guard_settings (
            channel_login TEXT PRIMARY KEY, channel_user_id TEXT, enabled BOOLEAN DEFAULT FALSE)",
        "CREATE TABLE twitch_smalltalk_messages (
            id BIGSERIAL PRIMARY KEY, channel_login TEXT NOT NULL, channel_user_id TEXT,
            session_id BIGINT, triggered_by_msg_id TEXT)",
        "CREATE TABLE twitch_channel_match_state (
            channel_login TEXT PRIMARY KEY, channel_user_id TEXT)",
        "CREATE TABLE twitch_chat_word_groups (
            id BIGSERIAL PRIMARY KEY, streamer_login TEXT NOT NULL, twitch_user_id TEXT)",
        "CREATE TABLE twitch_scout_pitch_blacklist (
            streamer_login TEXT PRIMARY KEY, twitch_user_id TEXT)",
        "CREATE TABLE twitch_scout_pitch_ledger (
            id BIGSERIAL PRIMARY KEY, streamer_login TEXT NOT NULL, twitch_user_id TEXT)",
        "CREATE TABLE twitch_promo_cooldowns (
            login TEXT NOT NULL, cooldown_type TEXT NOT NULL, twitch_user_id TEXT,
            PRIMARY KEY (login, cooldown_type))",

        "CREATE TABLE twitch_live_state (
            twitch_user_id TEXT PRIMARY KEY,
            streamer_login TEXT NOT NULL,
            last_stream_id TEXT, last_started_at TEXT, last_title TEXT, last_game_id TEXT,
            last_discord_message_id TEXT, last_notified_at TEXT,
            is_live INTEGER DEFAULT 0, last_seen_at TEXT, last_game TEXT,
            last_viewer_count INTEGER DEFAULT 0, last_tracking_token TEXT,
            active_session_id BIGINT, had_deadlock_in_session INTEGER DEFAULT 0,
            last_deadlock_seen_at TEXT
        )",
        "CREATE TABLE twitch_partners (
            id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT NOT NULL,
            twitch_login TEXT NOT NULL, status TEXT NOT NULL,
            raid_bot_enabled INTEGER DEFAULT 0,
            admin_archived_at TEXT,
            inactivity_flagged_at TEXT
        )",
        "CREATE TABLE twitch_stream_sessions (
            id BIGSERIAL PRIMARY KEY,
            streamer_login TEXT NOT NULL,
            -- 20260802140000: stabile Kanal-ID, nullable solange der Backfill
            -- eine Restmenge lässt (Prod 2026-08-02: 8393 von 9325 Zeilen).
            twitch_user_id TEXT,
            stream_id TEXT,
            -- P2.38: Prod-Spalten sind TEXT (ISO), nicht TIMESTAMPTZ —
            -- Fixture spiegelt das, sonst lügt sie gegen die Baseline.
            started_at TEXT NOT NULL,
            ended_at TEXT,
            duration_seconds INTEGER DEFAULT 0,
            start_viewers INTEGER DEFAULT 0, peak_viewers INTEGER DEFAULT 0,
            end_viewers INTEGER DEFAULT 0,
            avg_viewers DOUBLE PRECISION DEFAULT 0, samples INTEGER DEFAULT 0,
            retention_5m DOUBLE PRECISION, retention_10m DOUBLE PRECISION,
            retention_20m DOUBLE PRECISION,
            dropoff_pct DOUBLE PRECISION, dropoff_label TEXT,
            unique_chatters INTEGER DEFAULT 0, first_time_chatters INTEGER DEFAULT 0,
            returning_chatters INTEGER DEFAULT 0,
            followers_start INTEGER, followers_end INTEGER, follower_delta INTEGER,
            stream_title TEXT, notification_text TEXT, language TEXT,
            is_mature BOOLEAN DEFAULT FALSE, tags TEXT,
            had_deadlock_in_session BOOLEAN DEFAULT FALSE,
            game_name TEXT, notes TEXT
        )",
        "CREATE TABLE twitch_session_viewers (
            session_id BIGINT NOT NULL, ts_utc TIMESTAMPTZ NOT NULL,
            minutes_from_start INTEGER, viewer_count INTEGER NOT NULL,
            PRIMARY KEY (session_id, ts_utc)
        )",
        "CREATE TABLE twitch_session_chatters (
            session_id BIGINT NOT NULL, streamer_login TEXT NOT NULL,
            chatter_login TEXT NOT NULL, is_first_time_streamer BOOLEAN DEFAULT FALSE,
            confirmed_first_ever BOOLEAN DEFAULT FALSE
        )",
        "CREATE TABLE twitch_stats_tracked (
            ts_utc TIMESTAMPTZ, streamer TEXT, viewer_count INTEGER,
            is_partner BOOLEAN DEFAULT FALSE, game_name TEXT, stream_title TEXT, tags TEXT,
            language TEXT
        )",
        "CREATE TABLE twitch_stats_category (
            ts_utc TIMESTAMPTZ, streamer TEXT, viewer_count INTEGER,
            is_partner BOOLEAN DEFAULT FALSE, game_name TEXT, stream_title TEXT, tags TEXT,
            language TEXT
        )",
        "CREATE TABLE exp_sessions (
            id BIGSERIAL PRIMARY KEY, streamer TEXT NOT NULL, stream_id TEXT,
            started_at TEXT NOT NULL, ended_at TEXT, game_name TEXT, stream_title TEXT,
            peak_viewers INTEGER DEFAULT 0, avg_viewers REAL DEFAULT 0,
            samples INTEGER DEFAULT 0, follower_delta INTEGER, duration_min REAL
        )",
        "CREATE UNIQUE INDEX idx_exp_sessions_stream_id ON exp_sessions(stream_id)
            WHERE stream_id IS NOT NULL",
        "CREATE TABLE exp_snapshots (
            id BIGSERIAL PRIMARY KEY, exp_session_id BIGINT NOT NULL, ts_utc TEXT NOT NULL,
            viewer_count INTEGER, minutes_from_start REAL
        )",
        "CREATE UNIQUE INDEX idx_exp_snapshots_session_ts
            ON exp_snapshots(exp_session_id, ts_utc)",
        "CREATE TABLE exp_game_transitions (
            id BIGSERIAL PRIMARY KEY, exp_session_id BIGINT NOT NULL, streamer TEXT NOT NULL,
            ts_utc TEXT NOT NULL, from_game TEXT, to_game TEXT, viewer_count INTEGER
        )",
        // Poll-Loop-Infrastruktur: Partner-State-View als Tabelle nachgebildet.
        "CREATE TABLE twitch_streamers_partner_state (
            twitch_login TEXT PRIMARY KEY,
            twitch_user_id TEXT,
            require_discord_link INTEGER DEFAULT 0,
            archived_at TIMESTAMPTZ,
            is_partner_active INTEGER DEFAULT 0,
            is_partner INTEGER DEFAULT 0,
            operational_state TEXT DEFAULT 'active',
            discord_user_id TEXT,
            live_ping_role_id BIGINT,
            live_ping_enabled INTEGER DEFAULT 1
        )",
        "CREATE TABLE twitch_streamers (
            twitch_login TEXT PRIMARY KEY,
            twitch_user_id TEXT
        )",
        "CREATE UNIQUE INDEX idx_twitch_streamers_user_id ON twitch_streamers(twitch_user_id)",
        // Discord-Identitäts-Join der Tracking-Query (poller/tracked.rs) — muss
        // im Fixture existieren, sonst schlägt der LEFT JOIN mit 42P01 fehl.
        "CREATE TABLE twitch_streamer_identities (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT NOT NULL,
            discord_user_id TEXT,
            discord_display_name TEXT,
            is_on_discord INTEGER DEFAULT 0,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT DEFAULT CURRENT_TIMESTAMP
        )",
        "CREATE UNIQUE INDEX idx_twitch_streamer_identities_login_lower
            ON twitch_streamer_identities(LOWER(twitch_login))",
        "CREATE TABLE twitch_exclusions (
            twitch_user_id TEXT PRIMARY KEY,
            kind TEXT NOT NULL CHECK (kind IN ('opt_out', 'banned')),
            reason TEXT,
            excluded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            reactivated_at TIMESTAMPTZ
        )",
        // Raid-Auth-Store: Existenz einer Zeile = has_raid_auth (B8-07-RECONCILE).
        "CREATE TABLE twitch_raid_auth (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login   TEXT,
            needs_reauth   BOOLEAN NOT NULL DEFAULT FALSE
        )",
        "CREATE UNIQUE INDEX idx_twitch_raid_auth_login
            ON twitch_raid_auth(LOWER(twitch_login))",
        "CREATE TABLE twitch_partner_raid_scores (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT NOT NULL,
            final_score DOUBLE PRECISION NOT NULL DEFAULT 0.5
        )",
        "CREATE TABLE twitch_engagement_settings (
            channel_login TEXT PRIMARY KEY,
            enabled BOOLEAN NOT NULL DEFAULT FALSE
        )",
        "CREATE TABLE twitch_engagement_channel_profile (
            channel_login TEXT PRIMARY KEY,
            profile_text TEXT NOT NULL,
            msg_count INTEGER NOT NULL DEFAULT 0
        )",
        "CREATE TABLE twitch_streamer_invites (
            streamer_login TEXT PRIMARY KEY,
            twitch_user_id TEXT,
            invite_code TEXT NOT NULL UNIQUE,
            invite_url TEXT NOT NULL
        )",
        "CREATE TABLE twitch_raw_chat_ingest_health (
            streamer_login TEXT PRIMARY KEY,
            twitch_user_id TEXT,
            last_raw_chat_error TEXT,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
        "CREATE TABLE twitch_login_aliases (
            twitch_user_id TEXT NOT NULL,
            login TEXT NOT NULL,
            first_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            is_current BOOLEAN NOT NULL DEFAULT FALSE,
            PRIMARY KEY (twitch_user_id, login)
        )",
        "CREATE UNIQUE INDEX twitch_login_aliases_current_user_idx
            ON twitch_login_aliases(twitch_user_id) WHERE is_current",
        "CREATE TABLE twitch_global_settings (
            setting_key TEXT PRIMARY KEY,
            setting_value TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
        "CREATE TABLE eventsub_guard_state (
            kind TEXT NOT NULL,
            guard_key TEXT NOT NULL,
            expires_at DOUBLE PRECISION NOT NULL,
            updated_at DOUBLE PRECISION NOT NULL,
            PRIMARY KEY (kind, guard_key)
        )",
        "CREATE TABLE twitch_eventsub_processing_inbox (
            work_id          TEXT PRIMARY KEY,
            work_type        TEXT NOT NULL,
            message_id       TEXT,
            payload_json     TEXT NOT NULL,
            queued_at        DOUBLE PRECISION NOT NULL,
            next_attempt_at  DOUBLE PRECISION NOT NULL,
            attempt_count    INTEGER NOT NULL DEFAULT 0,
            last_error       TEXT
        )",
        "CREATE TABLE twitch_eventsub_processing_dead_letter (
            work_id           TEXT PRIMARY KEY,
            work_type         TEXT NOT NULL,
            message_id        TEXT,
            payload_json      TEXT NOT NULL,
            queued_at         DOUBLE PRECISION NOT NULL,
            dead_lettered_at  DOUBLE PRECISION NOT NULL,
            attempt_count     INTEGER NOT NULL,
            last_error        TEXT
        )",
        // Telemetrie-Event-Tabellen (Prod-Typen verifiziert 2026-06-09).
        // is_gift/is_automatic = BOOLEAN gemäß Produktionsvertrag
        // (migrations/20260619010000_runtime_type_contract.sql) — die Rust-
        // Schreibpfade binden bool. Fixtures dürfen hier NICHT auf INTEGER lügen
        // (P1.25 „Fixtures lügen gg. Baseline").
        "CREATE TABLE twitch_subscription_events (
            id BIGSERIAL PRIMARY KEY, session_id BIGINT, twitch_user_id TEXT,
            event_type TEXT, user_login TEXT, tier TEXT, is_gift BOOLEAN DEFAULT FALSE,
            gifter_login TEXT, cumulative_months INTEGER, streak_months INTEGER,
            message TEXT, total_gifted INTEGER, received_at TIMESTAMPTZ
        )",
        "CREATE TABLE twitch_ad_break_events (
            id BIGSERIAL PRIMARY KEY, session_id BIGINT, twitch_user_id TEXT,
            duration_seconds INTEGER, is_automatic BOOLEAN DEFAULT FALSE, started_at TIMESTAMPTZ
        )",
        "CREATE TABLE twitch_bits_events (
            id BIGSERIAL PRIMARY KEY, session_id BIGINT, twitch_user_id TEXT,
            donor_login TEXT, amount INTEGER, message TEXT, received_at TIMESTAMPTZ
        )",
        "CREATE TABLE twitch_channel_points_events (
            id BIGSERIAL PRIMARY KEY, session_id BIGINT, twitch_user_id TEXT,
            user_login TEXT, reward_id TEXT, reward_title TEXT, reward_cost INTEGER,
            user_input TEXT, redeemed_at TIMESTAMPTZ
        )",
        "CREATE TABLE twitch_hype_train_events (
            id BIGSERIAL PRIMARY KEY, session_id BIGINT, twitch_user_id TEXT,
            started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ, duration_seconds INTEGER,
            level INTEGER, total_progress INTEGER, event_phase TEXT
        )",
        "CREATE TABLE twitch_ban_events (
            id BIGSERIAL PRIMARY KEY, session_id BIGINT, twitch_user_id TEXT,
            event_type TEXT, target_login TEXT, target_id TEXT,
            moderator_login TEXT, reason TEXT, ends_at TIMESTAMPTZ, received_at TIMESTAMPTZ
        )",
        "CREATE TABLE twitch_chatter_global_ban (
            chatter_login TEXT PRIMARY KEY,
            chatter_id TEXT,
            reason TEXT,
            added_by TEXT,
            added_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
        "CREATE TABLE twitch_chatter_global_ban_applied (
            chatter_login TEXT NOT NULL,
            broadcaster_id TEXT NOT NULL,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            PRIMARY KEY (chatter_login, broadcaster_id)
        )",
        "CREATE TABLE twitch_shoutout_events (
            id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT, direction TEXT,
            other_broadcaster_id TEXT, other_broadcaster_login TEXT,
            moderator_login TEXT, viewer_count INTEGER, received_at TIMESTAMPTZ
        )",
        "CREATE TABLE twitch_follow_events (
            id BIGSERIAL PRIMARY KEY, streamer_login TEXT, twitch_user_id TEXT,
            follower_login TEXT, follower_id TEXT, followed_at TIMESTAMPTZ
        )",
        "CREATE TABLE twitch_first_message_events (
            id BIGSERIAL PRIMARY KEY, streamer_login TEXT, broadcaster_id TEXT,
            chatter_login TEXT, chatter_id TEXT, message_id TEXT,
            message_text TEXT, event_ts TIMESTAMPTZ
        )",
        "CREATE TABLE twitch_channel_updates (
            id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT, title TEXT,
            game_name TEXT, language TEXT, recorded_at TIMESTAMPTZ
        )",
        "CREATE TABLE twitch_live_announcement_configs (
            streamer_login TEXT PRIMARY KEY,
            config_json TEXT
        )",
        "CREATE TABLE twitch_eventsub_capacity_snapshot (
            id BIGSERIAL PRIMARY KEY, ts_utc TIMESTAMPTZ, trigger_reason TEXT,
            listener_count INTEGER, ready_listeners INTEGER, failed_listeners INTEGER,
            used_slots INTEGER, total_slots INTEGER, headroom_slots INTEGER,
            listeners_at_limit INTEGER, utilization_pct DOUBLE PRECISION,
            listeners_json TEXT
        )",
        "ALTER TABLE twitch_engagement_settings ADD COLUMN IF NOT EXISTS channel_user_id TEXT",
        "ALTER TABLE twitch_engagement_channel_profile ADD COLUMN IF NOT EXISTS channel_user_id TEXT",
        "ALTER TABLE twitch_live_announcement_configs ADD COLUMN IF NOT EXISTS twitch_user_id TEXT",
    ] {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }
    Some(pool)
}

/// Prod-treues Fixture für die Chatters/Presence-Poller- und Raid-Retention-
/// Tests (#11). **Bewusst eigenständig** — es mutiert das geteilte
/// [`pool_in_schema`] NICHT (das spiegelt absichtlich die ältere/abweichende
/// Prod-Form von `twitch_stream_sessions`/`twitch_session_chatters`), sondern
/// legt in einem frischen Schema genau die Spalten/PKs an, die der Poller- und
/// Retention-Code erwartet:
///
/// - `twitch_session_chatters` mit voller Spaltenliste + `PK(session_id, chatter_login)`
/// - `twitch_chatter_rollup` mit `timestamptz`-Timestamps + `PK(streamer_login, chatter_login)`
/// - `twitch_viewer_presence_ticks` mit `PK(session_id, viewer_login, tick_at)`
/// - `twitch_stream_sessions` mit `started_at`/`ended_at` als `TIMESTAMPTZ` (Prod!)
/// - `twitch_raid_history` + `twitch_raid_retention` (`target_session_id int4`)
/// - `twitch_live_state` + `twitch_streamers_partner_state` für den Roster-Join.
#[allow(dead_code)]
pub async fn pool_with_chatters_schema(schema: &str) -> Option<PgPool> {
    let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
        if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
            panic!(
                "TB_TEST_REQUIRE_DB=1 ist gesetzt, aber TB_TEST_DATABASE_URL fehlt — \
                 `rust/scripts/test_db.sh up` ausführen"
            );
        }
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt — `rust/scripts/test_db.sh up`");
        return None;
    };
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("admin connect");
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;

    let opts = PgConnectOptions::from_str(&dsn)
        .expect("dsn parse")
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .expect("connect");

    for ddl in [
        "CREATE TABLE twitch_live_state (
            twitch_user_id TEXT PRIMARY KEY,
            streamer_login TEXT NOT NULL,
            is_live INTEGER DEFAULT 0,
            last_seen_at TEXT,
            active_session_id BIGINT
        )",
        "CREATE TABLE twitch_streamers_partner_state (
            twitch_login TEXT PRIMARY KEY,
            twitch_user_id TEXT,
            is_partner_active INTEGER DEFAULT 0
        )",
        "CREATE TABLE twitch_session_chatters (
            session_id BIGINT NOT NULL,
            streamer_login TEXT NOT NULL,
            chatter_login TEXT NOT NULL,
            chatter_id TEXT,
            first_message_at TIMESTAMPTZ NOT NULL,
            messages INTEGER NOT NULL DEFAULT 0,
            is_first_time_streamer BOOLEAN NOT NULL DEFAULT FALSE,
            seen_via_chatters_api BOOLEAN NOT NULL DEFAULT FALSE,
            last_seen_at TIMESTAMPTZ,
            confirmed_first_ever BOOLEAN NOT NULL DEFAULT FALSE,
            PRIMARY KEY (session_id, chatter_login)
        )",
        "CREATE TABLE twitch_chatter_rollup (
            streamer_login TEXT NOT NULL,
            chatter_login TEXT NOT NULL,
            chatter_id TEXT,
            first_seen_at TIMESTAMPTZ NOT NULL,
            last_seen_at TIMESTAMPTZ NOT NULL,
            total_messages INTEGER NOT NULL DEFAULT 0,
            total_sessions INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (streamer_login, chatter_login)
        )",
        "CREATE TABLE twitch_viewer_presence_ticks (
            session_id BIGINT NOT NULL,
            streamer_login TEXT NOT NULL,
            viewer_login TEXT NOT NULL,
            tick_at TIMESTAMPTZ NOT NULL,
            PRIMARY KEY (session_id, viewer_login, tick_at)
        )",
        "CREATE TABLE twitch_stream_sessions (
            id BIGSERIAL PRIMARY KEY,
            streamer_login TEXT NOT NULL,
            started_at TIMESTAMPTZ NOT NULL,
            ended_at TIMESTAMPTZ
        )",
        "CREATE TABLE twitch_raid_history (
            id BIGINT NOT NULL,
            from_broadcaster_login TEXT NOT NULL,
            to_broadcaster_login TEXT NOT NULL,
            viewer_count INTEGER NOT NULL DEFAULT 0,
            executed_at TIMESTAMPTZ NOT NULL
        )",
        "CREATE TABLE twitch_raid_retention (
            raid_id BIGINT NOT NULL,
            from_broadcaster_login TEXT NOT NULL,
            to_broadcaster_login TEXT NOT NULL,
            viewer_count_sent INTEGER NOT NULL,
            executed_at TIMESTAMPTZ NOT NULL,
            target_session_id INTEGER,
            chatters_at_plus5m INTEGER,
            chatters_at_plus15m INTEGER,
            chatters_at_plus30m INTEGER,
            known_from_raider INTEGER,
            new_to_target INTEGER,
            new_chatters INTEGER,
            computed_at TIMESTAMPTZ DEFAULT NOW(),
            PRIMARY KEY (raid_id, executed_at)
        )",
    ] {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }
    Some(pool)
}
