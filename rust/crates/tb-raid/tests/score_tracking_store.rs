//! Hermetischer Test des Score-Tracking-Stores (`twitch_partner_raid_score_tracking`).

use std::str::FromStr;

use chrono::Utc;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_raid::{ScoreTrackingStore, TrackConfirmedInput};

macro_rules! pool_or_skip {
    ($schema:expr) => {{
        let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
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
    sqlx::query(
        "CREATE TABLE twitch_partner_raid_score_tracking (
            id SERIAL PRIMARY KEY, raid_history_id BIGINT, from_broadcaster_id TEXT,
            from_broadcaster_login TEXT, to_broadcaster_id TEXT, to_broadcaster_login TEXT,
            viewer_count INTEGER, confirmed_at TEXT, target_session_id INTEGER,
            target_stream_started_at TEXT, score_last_computed_at TEXT, final_score DOUBLE PRECISION,
            base_score DOUBLE PRECISION, duration_score DOUBLE PRECISION,
            time_pattern_score DOUBLE PRECISION, new_partner_multiplier DOUBLE PRECISION,
            raid_boost_multiplier DOUBLE PRECISION, today_received_raids INTEGER,
            was_deadlock_at_raid INTEGER, deadlock_continued_until TEXT, deadlock_continued_sec INTEGER,
            resolved_at TEXT, resolution_reason TEXT, raid_history_executed_at TIMESTAMPTZ,
            readiness_score DOUBLE PRECISION, fairness_score DOUBLE PRECISION )",
    ).execute(&pool).await.unwrap();
    sqlx::query(
        "CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, \
         started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE twitch_channel_updates (twitch_user_id TEXT, title TEXT, game_name TEXT, \
         language TEXT, recorded_at TIMESTAMPTZ)",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

#[tokio::test]
async fn track_schreibt_zeile_mit_int_deadlock_flag_und_leere_id_none() {
    let pool = pool_or_skip!("t6e_scoretrack");
    let store = ScoreTrackingStore::new(pool.clone());

    // Leere to_id → None, nichts geschrieben.
    let mut input = TrackConfirmedInput {
        to_broadcaster_login: "DST".into(),
        from_broadcaster_login: "SRC".into(),
        viewer_count: 42,
        confirmed_at: Utc::now().to_rfc3339(),
        final_score: Some(0.87),
        was_deadlock_at_raid: true,
        raid_history_id: Some(7),
        ..Default::default()
    };
    assert!(store.track_confirmed(&input).await.unwrap().is_none());

    // Gültig → id zurück, Logins lowercased, Deadlock-Flag als INTEGER 1.
    input.to_broadcaster_id = "200".into();
    let id = store.track_confirmed(&input).await.unwrap().expect("id");
    assert!(id > 0);

    let (to_login, from_login, deadlock, final_score, rhid): (String, String, i32, Option<f64>, Option<i64>) =
        sqlx::query_as(
            "SELECT to_broadcaster_login, from_broadcaster_login, was_deadlock_at_raid, final_score, raid_history_id
             FROM twitch_partner_raid_score_tracking WHERE id=$1",
        )
        .bind(id as i32)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(to_login, "dst");
    assert_eq!(from_login, "src");
    assert_eq!(deadlock, 1, "was_deadlock_at_raid als INTEGER");
    assert_eq!(final_score, Some(0.87));
    assert_eq!(rhid, Some(7));

    // deadlock_continued / resolved starten NULL.
    let resolved: Option<String> = sqlx::query_scalar(
        "SELECT resolved_at FROM twitch_partner_raid_score_tracking WHERE id=$1",
    )
    .bind(id as i32)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(resolved.is_none());
}

/// Insert-Helfer für eine offene (resolved_at NULL) Tracking-Zeile.
#[allow(clippy::too_many_arguments)]
async fn insert_open_row(
    pool: &PgPool,
    to_id: &str,
    to_login: &str,
    confirmed_at: &str,
    target_session_id: Option<i32>,
    target_stream_started_at: Option<&str>,
    was_deadlock: i32,
) -> i32 {
    sqlx::query_scalar(
        "INSERT INTO twitch_partner_raid_score_tracking \
         (to_broadcaster_id, to_broadcaster_login, confirmed_at, target_session_id, \
          target_stream_started_at, was_deadlock_at_raid) \
         VALUES ($1,$2,$3,$4,$5,$6) RETURNING id",
    )
    .bind(to_id)
    .bind(to_login)
    .bind(confirmed_at)
    .bind(target_session_id)
    .bind(target_stream_started_at)
    .bind(was_deadlock)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn resolve_setzt_offene_zeile_auf_resolved() {
    let pool = pool_or_skip!("t6e_resolve");
    let store = ScoreTrackingStore::new(pool.clone());

    let started: chrono::DateTime<Utc> = "2026-06-15T18:00:00+00:00".parse().unwrap();
    let ended: chrono::DateTime<Utc> = "2026-06-15T22:00:00+00:00".parse().unwrap();
    sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at) VALUES (1,'dst',$1,$2)")
        .bind(started).bind(ended).execute(&pool).await.unwrap();

    // Deadlock-Raid um 18:30, kein Nicht-Deadlock-Channel-Update → session_ended.
    let row_id = insert_open_row(
        &pool, "200", "dst", "2026-06-15T18:30:00+00:00", Some(1), None, 1,
    )
    .await;
    // Fremde Session-Zeile bleibt unangetastet.
    let other_id = insert_open_row(
        &pool, "999", "other", "2026-06-15T18:30:00+00:00", Some(2), None, 1,
    )
    .await;

    let resolved = store
        .resolve_for_session(Some("200"), "dst", Some(1), Some(ended), "deadlock")
        .await;
    assert_eq!(resolved, 1, "genau die Session-1-Zeile aufgelöst");

    let (resolved_at, reason, secs): (Option<String>, Option<String>, Option<i32>) =
        sqlx::query_as(
            "SELECT resolved_at, resolution_reason, deadlock_continued_sec \
             FROM twitch_partner_raid_score_tracking WHERE id=$1",
        )
        .bind(row_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(resolved_at.is_some(), "resolved_at gesetzt");
    assert_eq!(reason.as_deref(), Some("session_ended"));
    // 18:30 → 22:00 = 3.5h = 12600s.
    assert_eq!(secs, Some(12600));

    let other_resolved: Option<String> = sqlx::query_scalar(
        "SELECT resolved_at FROM twitch_partner_raid_score_tracking WHERE id=$1",
    )
    .bind(other_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(other_resolved.is_none(), "fremde Session bleibt offen");
}

#[tokio::test]
async fn resolve_deadlock_endet_an_non_deadlock_channel_update() {
    let pool = pool_or_skip!("t6e_resolve_update");
    let store = ScoreTrackingStore::new(pool.clone());

    let started: chrono::DateTime<Utc> = "2026-06-15T18:00:00+00:00".parse().unwrap();
    let ended: chrono::DateTime<Utc> = "2026-06-15T22:00:00+00:00".parse().unwrap();
    sqlx::query("INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at) VALUES (1,'dst',$1,$2)")
        .bind(started).bind(ended).execute(&pool).await.unwrap();
    let row_id = insert_open_row(
        &pool, "200", "dst", "2026-06-15T18:30:00+00:00", Some(1), None, 1,
    )
    .await;

    // Wechsel auf Nicht-Deadlock um 19:00 → resolution_dt = 19:00.
    let update_ts: chrono::DateTime<Utc> = "2026-06-15T19:00:00+00:00".parse().unwrap();
    sqlx::query("INSERT INTO twitch_channel_updates (twitch_user_id, game_name, recorded_at) VALUES ('200','Just Chatting',$1)")
        .bind(update_ts).execute(&pool).await.unwrap();

    let resolved = store
        .resolve_for_session(Some("200"), "dst", Some(1), Some(ended), "deadlock")
        .await;
    assert_eq!(resolved, 1);

    let (reason, secs): (Option<String>, Option<i32>) = sqlx::query_as(
        "SELECT resolution_reason, deadlock_continued_sec \
         FROM twitch_partner_raid_score_tracking WHERE id=$1",
    )
    .bind(row_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reason.as_deref(), Some("channel_update_non_deadlock"));
    // 18:30 → 19:00 = 1800s (nicht bis Session-Ende).
    assert_eq!(secs, Some(1800));
}

#[tokio::test]
async fn resolve_ohne_session_id_oder_ended_macht_nichts() {
    let pool = pool_or_skip!("t6e_resolve_noop");
    let store = ScoreTrackingStore::new(pool.clone());
    let ended: chrono::DateTime<Utc> = "2026-06-15T22:00:00+00:00".parse().unwrap();
    assert_eq!(
        store.resolve_for_session(Some("200"), "dst", None, Some(ended), "deadlock").await,
        0
    );
    assert_eq!(
        store.resolve_for_session(Some("200"), "dst", Some(1), None, "deadlock").await,
        0
    );
}
