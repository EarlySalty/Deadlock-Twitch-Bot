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
