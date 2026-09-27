use std::str::FromStr;

use chrono::{TimeZone, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_raid::{MonthlyRaidBoostStore, SeasonCloseOutcome};

async fn pool_or_skip(schema: &str) -> Option<PgPool> {
    let dsn = match std::env::var("TB_TEST_DATABASE_URL") {
        Ok(value) => value,
        Err(_) => {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return None;
        }
    };

    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
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

    let options = PgConnectOptions::from_str(&dsn)
        .unwrap()
        .options([("search_path", schema)]);
    Some(
        PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .unwrap(),
    )
}

async fn create_schema(pool: &PgPool) {
    sqlx::query(
        "CREATE TABLE twitch_partners (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            departnered_at TIMESTAMPTZ,
            admin_archived_at TIMESTAMPTZ,
            manual_partner_opt_out INTEGER NOT NULL DEFAULT 0,
            technical_pause_reason TEXT
        )",
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "CREATE TABLE partner_effort_events (
            id BIGSERIAL PRIMARY KEY,
            partner_twitch_user_id TEXT NOT NULL,
            partner_login TEXT NOT NULL,
            event_type TEXT NOT NULL,
            source_id TEXT NOT NULL,
            points INTEGER NOT NULL,
            occurred_at TIMESTAMPTZ NOT NULL
        )",
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "CREATE TABLE twitch_partner_effort_season_closures (
            season_key TEXT PRIMARY KEY,
            season_started_at TIMESTAMPTZ NOT NULL,
            season_ended_at TIMESTAMPTZ NOT NULL,
            closed_at TIMESTAMPTZ NOT NULL
        )",
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "CREATE TABLE twitch_partner_effort_season_results (
            season_key TEXT NOT NULL REFERENCES twitch_partner_effort_season_closures(season_key),
            twitch_user_id TEXT NOT NULL,
            twitch_login TEXT NOT NULL DEFAULT '',
            rank INTEGER NOT NULL,
            points BIGINT NOT NULL,
            qualified_invites BIGINT NOT NULL DEFAULT 0,
            score_reached_at TIMESTAMPTZ,
            closed_at TIMESTAMPTZ NOT NULL,
            PRIMARY KEY (season_key, twitch_user_id),
            UNIQUE (season_key, rank)
        )",
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "CREATE TABLE twitch_partner_raid_boost_grants (
            id BIGSERIAL PRIMARY KEY,
            season_key TEXT NOT NULL UNIQUE REFERENCES twitch_partner_effort_season_closures(season_key),
            twitch_user_id TEXT NOT NULL,
            twitch_login TEXT NOT NULL DEFAULT '',
            multiplier DOUBLE PRECISION NOT NULL DEFAULT 1.15,
            streams_total SMALLINT NOT NULL DEFAULT 2,
            streams_remaining SMALLINT NOT NULL DEFAULT 2,
            granted_at TIMESTAMPTZ NOT NULL,
            expires_at TIMESTAMPTZ NOT NULL
        )",
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "CREATE TABLE twitch_partner_raid_boost_streams (
            id BIGSERIAL PRIMARY KEY,
            grant_id BIGINT NOT NULL REFERENCES twitch_partner_raid_boost_grants(id),
            twitch_user_id TEXT NOT NULL,
            session_id BIGINT NOT NULL,
            stream_started_at TIMESTAMPTZ NOT NULL,
            reserved_at TIMESTAMPTZ NOT NULL,
            stream_ended_at TIMESTAMPTZ,
            deadlock_seconds INTEGER,
            qualified BOOLEAN,
            consumed_at TIMESTAMPTZ,
            UNIQUE (twitch_user_id, session_id)
        )",
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "CREATE TABLE twitch_live_state (
            twitch_user_id TEXT PRIMARY KEY,
            is_live INTEGER NOT NULL DEFAULT 0,
            active_session_id BIGINT
        )",
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "CREATE TABLE twitch_stream_sessions (
            id BIGINT PRIMARY KEY,
            twitch_user_id TEXT,
            streamer_login TEXT NOT NULL,
            started_at TIMESTAMPTZ NOT NULL,
            ended_at TIMESTAMPTZ,
            game_name TEXT
        )",
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "CREATE TABLE twitch_channel_updates (
            twitch_user_id TEXT NOT NULL,
            game_name TEXT,
            recorded_at TIMESTAMPTZ NOT NULL
        )",
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_grant(
    pool: &PgPool,
    season_key: &str,
    twitch_user_id: &str,
    granted_at: chrono::DateTime<Utc>,
) -> i64 {
    sqlx::query(
        "INSERT INTO twitch_partner_effort_season_closures
             (season_key, season_started_at, season_ended_at, closed_at)
         VALUES ($1, $2 - INTERVAL '30 days', $2, $2)",
    )
    .bind(season_key)
    .bind(granted_at)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query_scalar(
        "INSERT INTO twitch_partner_raid_boost_grants
             (season_key, twitch_user_id, twitch_login, granted_at, expires_at)
         VALUES ($1, $2, LOWER($2), $3, $3 + INTERVAL '30 days')
         RETURNING id",
    )
    .bind(season_key)
    .bind(twitch_user_id)
    .bind(granted_at)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn set_live_session(
    pool: &PgPool,
    session_id: i64,
    twitch_user_id: &str,
    started_at: chrono::DateTime<Utc>,
    game_name: &str,
) {
    sqlx::query(
        "INSERT INTO twitch_stream_sessions
             (id, twitch_user_id, streamer_login, started_at, game_name)
         VALUES ($1, $2, LOWER($2), $3, $4)",
    )
    .bind(session_id)
    .bind(twitch_user_id)
    .bind(started_at)
    .bind(game_name)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO twitch_live_state (twitch_user_id, is_live, active_session_id)
         VALUES ($1, 1, $2)
         ON CONFLICT (twitch_user_id) DO UPDATE
           SET is_live = 1, active_session_id = EXCLUDED.active_session_id",
    )
    .bind(twitch_user_id)
    .bind(session_id)
    .execute(pool)
    .await
    .unwrap();
}

async fn end_session(
    pool: &PgPool,
    session_id: i64,
    twitch_user_id: &str,
    ended_at: chrono::DateTime<Utc>,
) {
    sqlx::query("UPDATE twitch_stream_sessions SET ended_at = $2 WHERE id = $1")
        .bind(session_id)
        .bind(ended_at)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE twitch_live_state
            SET is_live = 0, active_session_id = NULL
          WHERE twitch_user_id = $1",
    )
    .bind(twitch_user_id)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn season_close_ist_beim_zweiten_lauf_idempotent() {
    let Some(pool) = pool_or_skip("monthly_raid_boost_idempotence").await else {
        return;
    };
    create_schema(&pool).await;

    sqlx::query(
        "INSERT INTO twitch_partners
             (twitch_user_id, twitch_login, status)
         VALUES
             ('winner', 'Winner', 'active'),
             ('runner', 'Runner', 'active'),
             ('early', 'Early', 'active'),
             ('late', 'Late', 'active')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Gleiche Punktzahl. winner gewinnt über mehr qualified_invites.
    sqlx::query(
        "INSERT INTO partner_effort_events
             (partner_twitch_user_id, partner_login, event_type, source_id, points, occurred_at)
         VALUES
             ('winner', 'winner', 'qualified_invite', 'invite-1', 10, '2026-08-20T12:00:00Z'),
             ('runner', 'runner', 'co_stream', 'stream-1', 10, '2026-08-10T12:00:00Z'),
             ('early', 'early', 'co_stream', 'stream-2', 5, '2026-08-05T12:00:00Z'),
             ('late', 'late', 'co_stream', 'stream-3', 5, '2026-08-15T12:00:00Z')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // 00:05 Europe/Berlin am 1. September 2026.
    let now = Utc
        .with_ymd_and_hms(2026, 8, 31, 22, 5, 0)
        .single()
        .unwrap();
    let store = MonthlyRaidBoostStore::new(pool.clone());

    let first = store.close_previous_season(now).await.unwrap();
    assert!(matches!(
        first,
        SeasonCloseOutcome::Closed {
            ref season_key,
            partners: 4,
            ..
        } if season_key == "2026-08"
    ));

    let second = store.close_previous_season(now).await.unwrap();
    assert_eq!(
        second,
        SeasonCloseOutcome::AlreadyClosed {
            season_key: "2026-08".to_string()
        }
    );

    let closures: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partner_effort_season_closures")
            .fetch_one(&pool)
            .await
            .unwrap();
    let results: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partner_effort_season_results")
            .fetch_one(&pool)
            .await
            .unwrap();
    let grants: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partner_raid_boost_grants")
        .fetch_one(&pool)
        .await
        .unwrap();
    let winner: String = sqlx::query_scalar(
        "SELECT twitch_user_id
           FROM twitch_partner_effort_season_results
          WHERE season_key = '2026-08' AND rank = 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(closures, 1);
    assert_eq!(results, 4);
    assert_eq!(grants, 1);
    assert_eq!(winner, "winner");

    let (early_rank, late_rank): (i32, i32) = sqlx::query_as(
        "SELECT
             MAX(rank) FILTER (WHERE twitch_user_id = 'early')::int4,
             MAX(rank) FILTER (WHERE twitch_user_id = 'late')::int4
           FROM twitch_partner_effort_season_results
          WHERE season_key = '2026-08'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        early_rank < late_rank,
        "bei gleichem Score und Invite-Zähler gewinnt der frühere Score-Zeitpunkt"
    );

    pool.close().await;
}

#[tokio::test]
async fn grant_wird_nach_zwei_qualifizierenden_streams_verbraucht() {
    let Some(pool) = pool_or_skip("monthly_raid_boost_two_streams").await else {
        return;
    };
    create_schema(&pool).await;

    let granted_at = Utc.with_ymd_and_hms(2026, 9, 1, 0, 5, 0).single().unwrap();
    insert_grant(&pool, "grant-two", "winner", granted_at).await;
    let store = MonthlyRaidBoostStore::new(pool.clone());

    for (session_id, hour) in [(101_i64, 10_u32), (102_i64, 12_u32)] {
        let started_at = Utc
            .with_ymd_and_hms(2026, 9, 2, hour, 0, 0)
            .single()
            .unwrap();
        set_live_session(&pool, session_id, "winner", started_at, "Deadlock").await;

        let active = store
            .reconcile_partner(
                "winner",
                "winner",
                started_at + chrono::Duration::minutes(1),
            )
            .await
            .unwrap();
        assert!(active.stream_boost_active);

        end_session(
            &pool,
            session_id,
            "winner",
            started_at + chrono::Duration::minutes(31),
        )
        .await;
        let ended = store
            .reconcile_partner(
                "winner",
                "winner",
                started_at + chrono::Duration::minutes(32),
            )
            .await
            .unwrap();
        assert!(ended.consumed_stream);
        assert!(!ended.stream_boost_active);
    }

    let remaining: i16 = sqlx::query_scalar(
        "SELECT streams_remaining
           FROM twitch_partner_raid_boost_grants
          WHERE twitch_user_id = 'winner'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining, 0);

    let third_start = Utc.with_ymd_and_hms(2026, 9, 3, 10, 0, 0).single().unwrap();
    set_live_session(&pool, 103, "winner", third_start, "Deadlock").await;
    let third = store
        .reconcile_partner(
            "winner",
            "winner",
            third_start + chrono::Duration::minutes(1),
        )
        .await
        .unwrap();
    assert!(!third.stream_boost_active);

    pool.close().await;
}

#[tokio::test]
async fn kurzer_stream_verbraucht_nicht_und_abgelaufener_grant_startet_nicht() {
    let Some(pool) = pool_or_skip("monthly_raid_boost_short_expiry").await else {
        return;
    };
    create_schema(&pool).await;

    let granted_at = Utc.with_ymd_and_hms(2026, 9, 1, 0, 5, 0).single().unwrap();
    insert_grant(&pool, "grant-short", "winner", granted_at).await;
    let store = MonthlyRaidBoostStore::new(pool.clone());

    let short_start = Utc.with_ymd_and_hms(2026, 9, 2, 10, 0, 0).single().unwrap();
    set_live_session(&pool, 201, "winner", short_start, "Deadlock").await;
    assert!(
        store
            .reconcile_partner(
                "winner",
                "winner",
                short_start + chrono::Duration::minutes(1)
            )
            .await
            .unwrap()
            .stream_boost_active
    );
    end_session(
        &pool,
        201,
        "winner",
        short_start + chrono::Duration::minutes(29),
    )
    .await;
    let short_end = store
        .reconcile_partner(
            "winner",
            "winner",
            short_start + chrono::Duration::minutes(30),
        )
        .await
        .unwrap();
    assert!(!short_end.consumed_stream);

    let remaining: i16 = sqlx::query_scalar(
        "SELECT streams_remaining
           FROM twitch_partner_raid_boost_grants
          WHERE twitch_user_id = 'winner'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining, 2);

    let expiry = granted_at + chrono::Duration::days(30);
    set_live_session(&pool, 202, "winner", expiry, "Deadlock").await;
    let expired = store
        .reconcile_partner("winner", "winner", expiry + chrono::Duration::minutes(1))
        .await
        .unwrap();
    assert!(!expired.stream_boost_active);

    pool.close().await;
}
