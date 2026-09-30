#[path = "../../../test-support/database.rs"]
mod test_database;
use std::str::FromStr;

use chrono::{TimeZone, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Connection, PgConnection, PgPool};
use tb_raid::{MonthlyRaidBoostStore, SeasonCloseOutcome};

async fn pool_or_skip(schema: &str) -> Option<PgPool> {
    pool_or_skip_with_max(schema, 4).await
}

async fn pool_or_skip_with_max(schema: &str, max_connections: u32) -> Option<PgPool> {
    let Some(dsn) = test_database::database_url() else {
        assert!(
            !test_database::required(),
            "isolierte Testdatenbank muss konfiguriert sein"
        );
        eprintln!("SKIP: keine isolierte Testdatenbank konfiguriert");
        return None;
    };

    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP SCHEMA IF EXISTS {schema} CASCADE"
    )))
    .execute(&admin)
    .await
    .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;

    let options = PgConnectOptions::from_str(&dsn)
        .unwrap()
        .options([("search_path", schema)]);
    Some(
        PgPoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await
            .unwrap(),
    )
}

#[tokio::test]
async fn closer_wartet_auf_engine_lock_ohne_pool_slot_zu_belegen() {
    let Some(pool) = pool_or_skip_with_max("monthly_effort_pool_one", 1).await else {
        return;
    };
    create_schema(&pool).await;
    sqlx::query("UPDATE partner_effort_source_state SET successful_at='2027-03-01'")
        .execute(&pool)
        .await
        .unwrap();

    let mut engine_connection = PgConnection::connect_with(&pool.connect_options())
        .await
        .unwrap();
    let mut engine_lock = engine_connection.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(713219, 27)")
        .execute(&mut *engine_lock)
        .await
        .unwrap();

    let store = MonthlyRaidBoostStore::new(pool.clone());
    let now = Utc.with_ymd_and_hms(2027, 2, 1, 12, 0, 0).single().unwrap();
    let closer = tokio::spawn(async move { store.close_season_ending_at(now, now).await });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let probe = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&pool),
    )
    .await
    .expect("Closer darf beim Warten auf den Engine-Lock keinen einzigen Pool-Slot halten")
    .unwrap();
    assert_eq!(probe, 1);

    engine_lock.rollback().await.unwrap();
    assert!(matches!(
        closer.await.unwrap().unwrap(),
        SeasonCloseOutcome::Closed { winner: None, .. }
    ));
    pool.close().await;
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
            occurred_at TIMESTAMPTZ NOT NULL,
            credited_at TIMESTAMPTZ GENERATED ALWAYS AS (occurred_at) STORED
        )",
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::raw_sql("CREATE TABLE partner_effort_program(singleton boolean PRIMARY KEY,started_at timestamptz); INSERT INTO partner_effort_program VALUES(TRUE,'2026-08-01'); CREATE TABLE partner_effort_source_state(source text PRIMARY KEY,healthy boolean,successful_at timestamptz); INSERT INTO partner_effort_source_state SELECT source,TRUE,'2027-01-01'::timestamptz FROM unnest(ARRAY['invites','referrals','clips','shared_chat','steam_party','engine','category_collection']) source;")
        .execute(pool).await.unwrap();

    // Execute the shipped migration in this test's isolated search_path.
    let migration =
        include_str!("../../../migrations/20260926230500_monthly_effort_raid_boost.sql")
            .replace("public.", "");
    sqlx::raw_sql(sqlx::AssertSqlSafe(migration))
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
            id BIGSERIAL PRIMARY KEY,
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

#[tokio::test]
async fn catchup_waits_for_sources_and_uses_credit_month_without_backdating_grants() {
    let Some(pool) = pool_or_skip("monthly_effort_catchup").await else {
        return;
    };
    create_schema(&pool).await;
    sqlx::raw_sql("UPDATE partner_effort_program SET started_at='2026-09-20'; INSERT INTO twitch_partners(twitch_user_id,twitch_login) VALUES('101','alice'); ALTER TABLE partner_effort_events ALTER COLUMN credited_at DROP EXPRESSION; INSERT INTO partner_effort_events(partner_twitch_user_id,partner_login,event_type,source_id,points,occurred_at,credited_at) VALUES('101','alice','qualified_invite','late',10,'2026-09-30T21:59:00Z','2026-09-30T22:01:00Z'); UPDATE partner_effort_source_state SET healthy=FALSE WHERE source='invites';")
        .execute(&pool).await.unwrap();
    let now = Utc
        .with_ymd_and_hms(2026, 11, 2, 12, 0, 0)
        .single()
        .unwrap();
    let store = MonthlyRaidBoostStore::new(pool.clone());
    let cutoffs = store.due_season_cutoffs(now).await.unwrap();
    assert_eq!(cutoffs.len(), 2);
    assert!(matches!(
        store.close_season_ending_at(cutoffs[0], now).await.unwrap(),
        SeasonCloseOutcome::SourceUnavailable { .. }
    ));
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partner_effort_season_closures")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    sqlx::query("UPDATE partner_effort_source_state SET healthy=TRUE")
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        store.close_season_ending_at(cutoffs[0], now).await.unwrap(),
        SeasonCloseOutcome::Closed { winner: None, .. }
    ));
    assert!(
        matches!(store.close_season_ending_at(cutoffs[1],now).await.unwrap(), SeasonCloseOutcome::Closed { winner: Some(ref winner), .. } if winner.points==10)
    );
    let grant: (chrono::DateTime<Utc>, chrono::DateTime<Utc>) =
        sqlx::query_as("SELECT granted_at,expires_at FROM twitch_partner_raid_boost_grants")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(grant, (now, now + chrono::Duration::days(30)));
    assert!(matches!(
        store.close_season_ending_at(cutoffs[1], now).await.unwrap(),
        SeasonCloseOutcome::AlreadyClosed { .. }
    ));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partner_raid_boost_grants")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    pool.close().await;
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
             ('late', 'Late', 'active'),
             ('corrected', 'Corrected', 'active'),
             ('later10', 'Later10', 'active')",
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
             ('late', 'late', 'co_stream', 'stream-3', 5, '2026-08-15T12:00:00Z'),
             ('corrected', 'corrected', 'co_stream', 'corr-1', 10, '2026-08-01T12:00:00Z'),
             ('corrected', 'corrected', 'co_stream', 'corr-2', 5, '2026-08-20T12:00:00Z'),
             ('corrected', 'corrected', 'co_stream', 'corr-3', -5, '2026-08-25T12:00:00Z'),
             ('later10', 'later10', 'co_stream', 'later-1', 10, '2026-08-10T12:00:00Z')",
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
            partners: 6,
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
    assert_eq!(results, 6);
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

    let (corrected_rank, later10_rank): (i32, i32) = sqlx::query_as(
        "SELECT
             MAX(rank) FILTER (WHERE twitch_user_id = 'corrected')::int4,
             MAX(rank) FILTER (WHERE twitch_user_id = 'later10')::int4
           FROM twitch_partner_effort_season_results
          WHERE season_key = '2026-08'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        corrected_rank < later10_rank,
        "Korrekturpunkte müssen den Zeitpunkt des finalen kumulierten Scores respektieren"
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
    insert_grant(&pool, "2026-08", "winner", granted_at).await;
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
    insert_grant(&pool, "2026-08", "winner", granted_at).await;
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

#[tokio::test]
async fn monat_ohne_punkte_vergibt_keinen_grant() {
    let Some(pool) = pool_or_skip("monthly_raid_boost_zero_season").await else {
        return;
    };
    create_schema(&pool).await;

    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status)
         VALUES ('alpha', 'Alpha', 'active'), ('beta', 'Beta', 'active')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let now = Utc
        .with_ymd_and_hms(2026, 8, 31, 22, 5, 0)
        .single()
        .unwrap();
    let outcome = MonthlyRaidBoostStore::new(pool.clone())
        .close_previous_season(now)
        .await
        .unwrap();

    assert!(matches!(
        outcome,
        SeasonCloseOutcome::Closed {
            partners: 2,
            winner: None,
            ..
        }
    ));
    let grants: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partner_raid_boost_grants")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(grants, 0);

    pool.close().await;
}

#[tokio::test]
async fn paralleler_restart_kann_keinen_dritten_slot_reservieren() {
    let Some(pool) = pool_or_skip("monthly_raid_boost_restart_race").await else {
        return;
    };
    create_schema(&pool).await;

    let granted_at = Utc.with_ymd_and_hms(2026, 9, 1, 0, 5, 0).single().unwrap();
    let grant_id = insert_grant(&pool, "2026-08", "winner", granted_at).await;
    sqlx::query(
        "UPDATE twitch_partner_raid_boost_grants
            SET streams_remaining = 1
          WHERE id = $1",
    )
    .bind(grant_id)
    .execute(&pool)
    .await
    .unwrap();

    let store = MonthlyRaidBoostStore::new(pool.clone());
    let previous_start = Utc.with_ymd_and_hms(2026, 9, 2, 10, 0, 0).single().unwrap();
    set_live_session(&pool, 301, "winner", previous_start, "Deadlock").await;
    assert!(
        store
            .reconcile_partner(
                "winner",
                "winner",
                previous_start + chrono::Duration::minutes(1),
            )
            .await
            .unwrap()
            .stream_boost_active
    );

    end_session(
        &pool,
        301,
        "winner",
        previous_start + chrono::Duration::minutes(31),
    )
    .await;
    let next_start = previous_start + chrono::Duration::minutes(32);
    set_live_session(&pool, 302, "winner", next_start, "Deadlock").await;

    let first = store.clone();
    let second = store.clone();
    let (a, b) = tokio::join!(
        first.reconcile_partner(
            "winner",
            "winner",
            next_start + chrono::Duration::seconds(10)
        ),
        second.reconcile_partner(
            "winner",
            "winner",
            next_start + chrono::Duration::seconds(10)
        )
    );
    assert!(!a.unwrap().stream_boost_active);
    assert!(!b.unwrap().stream_boost_active);

    let remaining: i16 = sqlx::query_scalar(
        "SELECT streams_remaining FROM twitch_partner_raid_boost_grants WHERE id = $1",
    )
    .bind(grant_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let next_reservations: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_partner_raid_boost_streams WHERE session_id = 302",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining, 0);
    assert_eq!(next_reservations, 0);

    pool.close().await;
}

#[tokio::test]
async fn ueberlappende_grants_verbrauchen_den_frueher_ablaufenden_zuerst() {
    let Some(pool) = pool_or_skip("monthly_raid_boost_overlap").await else {
        return;
    };
    create_schema(&pool).await;

    let old_granted = Utc.with_ymd_and_hms(2026, 9, 1, 0, 5, 0).single().unwrap();
    let new_granted = Utc.with_ymd_and_hms(2026, 9, 20, 0, 5, 0).single().unwrap();
    let old_id = insert_grant(&pool, "2026-08", "winner", old_granted).await;
    let new_id = insert_grant(&pool, "2026-09", "winner", new_granted).await;

    let start = Utc
        .with_ymd_and_hms(2026, 9, 25, 10, 0, 0)
        .single()
        .unwrap();
    set_live_session(&pool, 401, "winner", start, "Deadlock").await;
    let state = MonthlyRaidBoostStore::new(pool.clone())
        .reconcile_partner("winner", "winner", start + chrono::Duration::minutes(1))
        .await
        .unwrap();

    assert_eq!(state.grant_id, Some(old_id));
    assert_ne!(state.grant_id, Some(new_id));

    pool.close().await;
}

#[tokio::test]
async fn laufender_stream_bleibt_bei_ablauf_aktiv_aber_neuer_stream_nicht() {
    let Some(pool) = pool_or_skip("monthly_boost_expiry_midstream").await else {
        return;
    };
    create_schema(&pool).await;
    let granted = Utc.with_ymd_and_hms(2026, 9, 1, 0, 5, 0).single().unwrap();
    let expiry = granted + chrono::Duration::days(30);
    let id = insert_grant(&pool, "2026-08", "winner", granted).await;
    let store = MonthlyRaidBoostStore::new(pool.clone());
    let start = expiry - chrono::Duration::minutes(1);
    set_live_session(&pool, 501, "winner", start, "Deadlock").await;
    assert!(
        store
            .reconcile_partner("winner", "winner", start)
            .await
            .unwrap()
            .stream_boost_active
    );
    // A recreated store simulates a process restart after expiry.
    let store = MonthlyRaidBoostStore::new(pool.clone());
    assert!(
        store
            .reconcile_partner("winner", "winner", expiry + chrono::Duration::minutes(40))
            .await
            .unwrap()
            .stream_boost_active
    );
    end_session(&pool, 501, "winner", expiry + chrono::Duration::minutes(40)).await;
    let state = store
        .reconcile_partner("winner", "winner", expiry + chrono::Duration::minutes(41))
        .await
        .unwrap();
    assert!(state.consumed_stream);
    assert!(!state.stream_boost_active);
    let replay = store
        .reconcile_partner("winner", "winner", expiry + chrono::Duration::minutes(41))
        .await
        .unwrap();
    assert!(!replay.consumed_stream);
    let remaining: i16 = sqlx::query_scalar(
        "SELECT streams_remaining FROM twitch_partner_raid_boost_grants WHERE id=$1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining, 1);
    set_live_session(
        &pool,
        502,
        "winner",
        expiry + chrono::Duration::hours(2),
        "Deadlock",
    )
    .await;
    assert!(
        !store
            .reconcile_partner("winner", "winner", expiry + chrono::Duration::hours(2))
            .await
            .unwrap()
            .stream_boost_active
    );
    pool.close().await;
}

#[tokio::test]
async fn fremde_session_und_recycelter_login_bekommen_keinen_grant() {
    let Some(pool) = pool_or_skip("monthly_boost_session_identity").await else {
        return;
    };
    create_schema(&pool).await;
    let granted = Utc.with_ymd_and_hms(2026, 9, 1, 0, 5, 0).single().unwrap();
    insert_grant(&pool, "2026-08", "winner", granted).await;
    let start = granted + chrono::Duration::days(1);
    set_live_session(&pool, 601, "another_id", start, "Deadlock").await;
    sqlx::query("UPDATE twitch_stream_sessions SET streamer_login='winner' WHERE id=601")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO twitch_live_state VALUES ('winner',1,601)")
        .execute(&pool)
        .await
        .unwrap();
    let store = MonthlyRaidBoostStore::new(pool.clone());
    assert!(
        !store
            .reconcile_partner("winner", "winner", start)
            .await
            .unwrap()
            .stream_boost_active
    );
    sqlx::query(
        "UPDATE twitch_live_state SET active_session_id=NULL WHERE twitch_user_id='winner'",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        !store
            .reconcile_partner("winner", "winner", start)
            .await
            .unwrap()
            .stream_boost_active
    );
    let reservations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partner_raid_boost_streams")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(reservations, 0);
    // Renaming the winner is safe: the ID, not the supplied login, owns the stream.
    set_live_session(&pool, 602, "winner", start, "Deadlock").await;
    assert!(
        store
            .reconcile_partner("winner", "renamed", start)
            .await
            .unwrap()
            .stream_boost_active
    );
    pool.close().await;
}

#[tokio::test]
async fn offener_alter_stream_reserviert_letzten_slot_bis_zum_abschluss() {
    let Some(pool) = pool_or_skip("monthly_boost_pending_slot").await else {
        return;
    };
    create_schema(&pool).await;
    let granted = Utc.with_ymd_and_hms(2026, 9, 1, 0, 5, 0).single().unwrap();
    let id = insert_grant(&pool, "2026-08", "winner", granted).await;
    sqlx::query("UPDATE twitch_partner_raid_boost_grants SET streams_remaining=1 WHERE id=$1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let start = granted + chrono::Duration::days(1);
    let store = MonthlyRaidBoostStore::new(pool.clone());
    set_live_session(&pool, 701, "winner", start, "Deadlock").await;
    assert!(
        store
            .reconcile_partner("winner", "winner", start)
            .await
            .unwrap()
            .stream_boost_active
    );
    // Simulate delayed finalization: live_state already points at the next stream.
    set_live_session(
        &pool,
        702,
        "winner",
        start + chrono::Duration::hours(1),
        "Deadlock",
    )
    .await;
    assert!(
        !store
            .reconcile_partner("winner", "winner", start + chrono::Duration::hours(1))
            .await
            .unwrap()
            .stream_boost_active
    );
    let reservations: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_partner_raid_boost_streams WHERE grant_id=$1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reservations, 1);
    pool.close().await;
}

#[tokio::test]
async fn kategorie_am_streamstart_zaehlt_und_plan_verschiebt_verbrauch_nicht() {
    let Some(pool) = pool_or_skip("monthly_boost_category_plan").await else {
        return;
    };
    create_schema(&pool).await;
    let granted = Utc.with_ymd_and_hms(2026, 9, 1, 0, 5, 0).single().unwrap();
    let id = insert_grant(&pool, "2026-08", "winner", granted).await;
    let start = granted + chrono::Duration::days(1);
    set_live_session(&pool, 801, "winner", start, "Just Chatting").await;
    sqlx::query("INSERT INTO twitch_channel_updates(twitch_user_id,recorded_at,game_name) VALUES ('winner',$1,'Deadlock')").bind(start).execute(&pool).await.unwrap();
    let store = MonthlyRaidBoostStore::new(pool.clone());
    let active = store
        .reconcile_partner("winner", "winner", start)
        .await
        .unwrap();
    assert!(active.stream_boost_active);
    let combined = tb_raid::combined_raid_boost_enabled(true, active.stream_boost_active);
    assert_eq!(
        tb_raid::compute_raid_boost_multiplier(combined),
        tb_raid::RAID_BOOST_MULTIPLIER
    );
    end_session(&pool, 801, "winner", start + chrono::Duration::minutes(30)).await;
    assert!(
        store
            .reconcile_partner("winner", "winner", start + chrono::Duration::minutes(30))
            .await
            .unwrap()
            .consumed_stream
    );
    let remaining: i16 = sqlx::query_scalar(
        "SELECT streams_remaining FROM twitch_partner_raid_boost_grants WHERE id=$1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining, 1);

    // Ein gelöschtes Spiel ist ein echter Wechsel zu einer unbekannten
    // Kategorie. Die restlichen Minuten dürfen keinen zweiten Slot verbrauchen.
    let second = start + chrono::Duration::days(1);
    set_live_session(&pool, 802, "winner", second, "Deadlock").await;
    assert!(
        store
            .reconcile_partner("winner", "winner", second)
            .await
            .unwrap()
            .stream_boost_active
    );
    sqlx::query("INSERT INTO twitch_channel_updates(twitch_user_id,recorded_at,game_name) VALUES('winner',$1,NULL)")
        .bind(second + chrono::Duration::minutes(20)).execute(&pool).await.unwrap();
    end_session(&pool, 802, "winner", second + chrono::Duration::minutes(60)).await;
    assert!(
        !store
            .reconcile_partner("winner", "winner", second + chrono::Duration::minutes(60))
            .await
            .unwrap()
            .consumed_stream
    );
    let remaining: i16 = sqlx::query_scalar(
        "SELECT streams_remaining FROM twitch_partner_raid_boost_grants WHERE id=$1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining, 1);
    pool.close().await;
}

#[tokio::test]
async fn paralleler_abschluss_ist_einmalig_und_ergebnisse_sind_unveraenderlich() {
    let Some(pool) = pool_or_skip("monthly_boost_concurrent_close").await else {
        return;
    };
    create_schema(&pool).await;
    sqlx::query(
        "INSERT INTO twitch_partners(twitch_user_id,twitch_login) VALUES ('winner','winner')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO partner_effort_events(partner_twitch_user_id,partner_login,event_type,source_id,points,occurred_at) VALUES ('winner','winner','qualified_invite','invite',10,'2026-08-20T10:00:00Z')").execute(&pool).await.unwrap();
    let now = Utc
        .with_ymd_and_hms(2026, 8, 31, 22, 5, 0)
        .single()
        .unwrap();
    let store = MonthlyRaidBoostStore::new(pool.clone());
    let (a, b) = tokio::join!(
        store.close_previous_season(now),
        store.close_previous_season(now)
    );
    let outcomes = [a.unwrap(), b.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|r| matches!(r, SeasonCloseOutcome::Closed { .. }))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|r| matches!(r, SeasonCloseOutcome::AlreadyClosed { .. }))
            .count(),
        1
    );
    let grants: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partner_raid_boost_grants")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(grants, 1);
    for sql in [
        "UPDATE twitch_partner_effort_season_results SET points=99",
        "DELETE FROM twitch_partner_effort_season_results",
        "TRUNCATE twitch_partner_effort_season_results",
        "UPDATE twitch_partner_effort_season_closures SET closed_at=now()",
    ] {
        let error = sqlx::query(sql).execute(&pool).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("55000")
        );
    }
    pool.close().await;
}

#[tokio::test]
async fn saison_ohne_partner_schliesst_ohne_grant() {
    let Some(pool) = pool_or_skip("monthly_boost_empty_season").await else {
        return;
    };
    create_schema(&pool).await;
    let now = Utc
        .with_ymd_and_hms(2026, 8, 31, 22, 5, 0)
        .single()
        .unwrap();
    let outcome = MonthlyRaidBoostStore::new(pool.clone())
        .close_previous_season(now)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        SeasonCloseOutcome::Closed {
            partners: 0,
            winner: None,
            ..
        }
    ));
    let grants: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partner_raid_boost_grants")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(grants, 0);
    pool.close().await;
}

#[tokio::test]
async fn read_only_boost_lesepfad_reserviert_und_verbraucht_nicht() {
    let schema = "monthly_boost_read_only";
    let Some(pool) = pool_or_skip(schema).await else {
        return;
    };
    create_schema(&pool).await;
    let granted = Utc.with_ymd_and_hms(2026, 9, 1, 0, 5, 0).single().unwrap();
    let id = insert_grant(&pool, "2026-08", "winner", granted).await;
    let start = granted + chrono::Duration::days(1);
    set_live_session(&pool, 901, "winner", start, "Deadlock").await;

    let options = PgConnectOptions::from_str(&test_database::database_url().unwrap())
        .unwrap()
        .options([
            ("search_path", schema),
            ("default_transaction_read_only", "on"),
        ]);
    let read_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    let reader = MonthlyRaidBoostStore::new(read_pool.clone());
    assert!(!reader.reserved_stream_boost_active("winner").await.unwrap());
    let reservations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partner_raid_boost_streams")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(reservations, 0);

    let writer = MonthlyRaidBoostStore::new(pool.clone());
    writer
        .reconcile_partner("winner", "winner", start)
        .await
        .unwrap();
    assert!(reader.reserved_stream_boost_active("winner").await.unwrap());
    assert!(!reader
        .reserved_stream_boost_active("another_id")
        .await
        .unwrap());
    end_session(&pool, 901, "winner", start + chrono::Duration::minutes(30)).await;
    assert!(!reader.reserved_stream_boost_active("winner").await.unwrap());
    let remaining: i16 = sqlx::query_scalar(
        "SELECT streams_remaining FROM twitch_partner_raid_boost_grants WHERE id=$1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        remaining, 2,
        "A read must not consume a completed qualifying stream"
    );
    assert!(
        writer
            .reconcile_partner("winner", "winner", start + chrono::Duration::minutes(30))
            .await
            .unwrap()
            .consumed_stream
    );
    read_pool.close().await;
    pool.close().await;
}
