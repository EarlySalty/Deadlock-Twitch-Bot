use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::Value;
use sqlx::{postgres::PgPoolOptions, PgPool};
use tower::ServiceExt;

use tb_dashboard_api::build_public_router;

fn test_dsn() -> Option<String> {
    std::env::var("TB_TEST_DATABASE_URL").ok()
}

macro_rules! db_dsn_or_skip {
    () => {
        match test_dsn() {
            Some(dsn) => dsn,
            None => {
                if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                    panic!("TB_TEST_REQUIRE_DB=1 ist gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                }
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        }
    };
}

async fn make_pool(dsn: &str, schema: &str) -> PgPool {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(dsn)
        .await
        .expect("connect test-db");

    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&pool)
        .await
        .expect("drop schema");
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&pool)
        .await
        .expect("create schema");
    sqlx::query(&format!("SET search_path TO {schema}"))
        .execute(&pool)
        .await
        .expect("set search_path");

    sqlx::raw_sql(
        r#"
        CREATE TABLE twitch_streamers_partner_state (
            twitch_login TEXT NOT NULL,
            is_partner_active INTEGER NOT NULL
        );

        CREATE TABLE twitch_stream_sessions (
            id BIGINT PRIMARY KEY,
            streamer_login TEXT NOT NULL,
            started_at TIMESTAMPTZ NOT NULL,
            ended_at TIMESTAMPTZ,
            avg_viewers DOUBLE PRECISION DEFAULT 0,
            peak_viewers INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE twitch_stats_tracked (
            ts_utc TIMESTAMPTZ NOT NULL,
            streamer TEXT NOT NULL,
            viewer_count INTEGER NOT NULL
        );

        CREATE TABLE twitch_raid_retention (
            raid_id BIGINT NOT NULL,
            from_broadcaster_login TEXT NOT NULL,
            to_broadcaster_login TEXT NOT NULL,
            viewer_count_sent INTEGER NOT NULL,
            executed_at TIMESTAMPTZ NOT NULL,
            target_session_id BIGINT
        );
        "#,
    )
    .execute(&pool)
    .await
    .expect("comparison fixture ddl");

    pool
}

async fn response_json(pool: PgPool, uri: &str) -> (StatusCode, Value) {
    let response = build_public_router(pool)
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json = serde_json::from_slice(&body).unwrap();
    (status, json)
}

#[tokio::test]
async fn public_comparison_aggregiert_fair_und_ohne_private_felder() {
    let dsn = db_dsn_or_skip!();
    let pool = make_pool(&dsn, "public_streamer_comparison_contract").await;

    sqlx::query(
        r#"
        INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active)
        VALUES ('Alpha', 1), ('beta', 1), ('paused', 0), ('no_stream', 1)
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT INTO twitch_stream_sessions
            (id, streamer_login, started_at, ended_at, avg_viewers, peak_viewers)
        VALUES
            (1, 'alpha', NOW() - INTERVAL '10 days', NOW() - INTERVAL '9 days 12 hours', 10, 20),
            (2, 'alpha', NOW() - INTERVAL '2 days 1 hour', NOW() - INTERVAL '1 day 13 hours', 20, 30),
            (3, 'beta', NOW() - INTERVAL '4 days', NOW() - INTERVAL '3 days 12 hours', 5, 8),
            (4, 'paused', NOW() - INTERVAL '3 days', NOW() - INTERVAL '2 days 12 hours', 100, 150)
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT INTO twitch_raid_retention
            (raid_id, from_broadcaster_login, to_broadcaster_login,
             viewer_count_sent, executed_at, target_session_id)
        VALUES (10, 'source', 'alpha', 10, NOW() - INTERVAL '2 days', 2)
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        WITH raid AS (SELECT NOW() - INTERVAL '2 days' AS at)
        INSERT INTO twitch_stats_tracked (ts_utc, streamer, viewer_count)
        SELECT at + minute * INTERVAL '1 minute', 'alpha',
               CASE
                   WHEN minute < -1 THEN 5
                   WHEN minute < 5 THEN 8
                   ELSE 7
               END
        FROM raid, generate_series(-10, 29) AS minute
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let (status, json) =
        response_json(pool, "/twitch/api/v2/public/streamer-comparison?days=30").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["period"]["days"], 30);
    assert_eq!(json["period"]["timezone"], "Europe/Berlin");
    assert_eq!(json["network"]["streamerCount"], 2);
    assert_eq!(json["network"]["confirmedRaids"], 1);
    assert_eq!(json["network"]["viewersForwarded"], 10);

    let streamers = json["streamers"].as_array().expect("streamers array");
    assert_eq!(
        streamers.len(),
        2,
        "inaktive/streamlose Kanaele bleiben draussen"
    );
    let alpha = streamers
        .iter()
        .find(|streamer| streamer["login"] == "alpha")
        .expect("alpha row");

    assert_eq!(alpha["sampleQualified"], true);
    assert_eq!(alpha["sessions"], 2);
    assert_eq!(alpha["streamHours"], 24.0);
    assert_eq!(alpha["averageViewers"], 15.0);
    assert_eq!(alpha["peakViewers"], 30);
    assert_eq!(alpha["viewerHours"], 360.0);
    assert_eq!(alpha["confirmedRaids"], 1);
    assert_eq!(alpha["raidViewersReceived"], 10);
    assert_eq!(alpha["measuredRaids"], 1);
    assert_eq!(alpha["raidUplift5m"], 3.0);
    assert_eq!(alpha["raidUplift30m"], 2.0);
    assert_eq!(alpha["ranks"]["averageViewers"], 1);
    assert_eq!(alpha["ranks"]["viewerHours"], 1);
    assert!(alpha["nextStep"]["code"].is_string());
    assert!(alpha["nextStep"]["reason"].is_string());

    let serialized = serde_json::to_string(&json).unwrap().to_lowercase();
    for private_field in [
        "discord_user_id",
        "subscriber",
        "subscription",
        "revenue",
        "earnings",
        "follower_login",
        "chatter_login",
        "viewer_login",
    ] {
        assert!(
            !serialized.contains(private_field),
            "privates Feld darf nicht oeffentlich sein: {private_field}"
        );
    }
}

#[tokio::test]
async fn public_comparison_akzeptiert_nur_feste_zeitraeume() {
    let dsn = db_dsn_or_skip!();
    let pool = make_pool(&dsn, "public_streamer_comparison_days").await;

    let (status, json) =
        response_json(pool, "/twitch/api/v2/public/streamer-comparison?days=13").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"], "invalid_days");
    assert_eq!(json["allowedDays"], serde_json::json!([7, 30, 90]));
}

#[tokio::test]
async fn public_comparison_cache_bleibt_auf_einen_router_begrenzt() {
    let dsn = db_dsn_or_skip!();
    let first_pool = make_pool(&dsn, "public_streamer_comparison_cache_first").await;
    let second_pool = make_pool(&dsn, "public_streamer_comparison_cache_second").await;

    for (pool, login, average_viewers) in
        [(&first_pool, "first", 4.0), (&second_pool, "second", 9.0)]
    {
        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active) VALUES ($1, 1)",
        )
        .bind(login)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO twitch_stream_sessions
                (id, streamer_login, started_at, ended_at, avg_viewers, peak_viewers)
            VALUES (1, $1, NOW() - INTERVAL '12 hours', NOW(), $2, $2::INTEGER)
            "#,
        )
        .bind(login)
        .bind(average_viewers)
        .execute(pool)
        .await
        .unwrap();
    }

    let (_, first) = response_json(
        first_pool,
        "/twitch/api/v2/public/streamer-comparison?days=30",
    )
    .await;
    let (_, second) = response_json(
        second_pool,
        "/twitch/api/v2/public/streamer-comparison?days=30",
    )
    .await;

    assert_eq!(first["streamers"][0]["login"], "first");
    assert_eq!(second["streamers"][0]["login"], "second");
    assert_eq!(first["streamers"][0]["averageViewers"], 4.0);
    assert_eq!(second["streamers"][0]["averageViewers"], 9.0);
}

#[tokio::test]
async fn public_comparison_behandelt_historische_null_durchschnitte_als_null_zuschauer() {
    let dsn = db_dsn_or_skip!();
    let pool = make_pool(&dsn, "public_streamer_comparison_null_average").await;

    sqlx::query(
        "INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active) VALUES ('nullavg', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO twitch_stream_sessions
            (id, streamer_login, started_at, ended_at, avg_viewers, peak_viewers)
        VALUES (1, 'nullavg', NOW() - INTERVAL '12 hours', NOW(), NULL, 0)
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let (status, json) =
        response_json(pool, "/twitch/api/v2/public/streamer-comparison?days=30").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["streamers"][0]["login"], "nullavg");
    assert_eq!(json["streamers"][0]["averageViewers"], 0.0);
    assert_eq!(json["streamers"][0]["viewerHours"], 0.0);
}

#[tokio::test]
async fn public_comparison_wertet_ueberlappende_raid_fenster_nicht_doppelt() {
    let dsn = db_dsn_or_skip!();
    let pool = make_pool(&dsn, "public_streamer_comparison_overlapping_raids").await;

    sqlx::query(
        "INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active) VALUES ('target', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO twitch_stream_sessions
            (id, streamer_login, started_at, ended_at, avg_viewers, peak_viewers)
        VALUES (1, 'target', NOW() - INTERVAL '3 days', NOW() - INTERVAL '1 day', 7, 12)
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        WITH first_raid AS (SELECT NOW() - INTERVAL '2 days' AS at)
        INSERT INTO twitch_raid_retention
            (raid_id, from_broadcaster_login, to_broadcaster_login,
             viewer_count_sent, executed_at, target_session_id)
        SELECT 20, 'source_a', 'target', 10, at, 1 FROM first_raid
        UNION ALL
        SELECT 21, 'source_b', 'target', 12, at + INTERVAL '10 minutes', 1 FROM first_raid
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        WITH first_raid AS (SELECT NOW() - INTERVAL '2 days' AS at)
        INSERT INTO twitch_stats_tracked (ts_utc, streamer, viewer_count)
        SELECT at + minute * INTERVAL '1 minute', 'target',
               CASE WHEN minute < 0 THEN 5 ELSE 10 END
        FROM first_raid, generate_series(-10, 49) AS minute
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let (status, json) =
        response_json(pool, "/twitch/api/v2/public/streamer-comparison?days=30").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["streamers"][0]["confirmedRaids"], 2);
    assert_eq!(json["streamers"][0]["measuredRaids"], 0);
    assert_eq!(json["streamers"][0]["raidUplift5m"], Value::Null);
    assert_eq!(json["streamers"][0]["raidUplift30m"], Value::Null);
}

#[tokio::test]
async fn public_comparison_erfindet_keinen_peak_fuer_hineinragende_altsession() {
    let dsn = db_dsn_or_skip!();
    let pool = make_pool(&dsn, "public_streamer_comparison_overlapping_session_peak").await;

    sqlx::query(
        "INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active) VALUES ('overnight', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO twitch_stream_sessions
            (id, streamer_login, started_at, ended_at, avg_viewers, peak_viewers)
        VALUES (1, 'overnight', NOW() - INTERVAL '8 days', NOW() - INTERVAL '6 days', 6, 99)
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let (status, json) =
        response_json(pool, "/twitch/api/v2/public/streamer-comparison?days=7").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["streamers"][0]["streamHours"], 24.0);
    assert_eq!(json["streamers"][0]["averageViewers"], 6.0);
    assert_eq!(json["streamers"][0]["peakViewers"], Value::Null);
}

#[tokio::test]
#[ignore = "benoetigt TB_LIVE_READONLY_DATABASE_URL und prueft nur das Live-Schema lesend"]
async fn public_comparison_live_schema_smoke_readonly() {
    let dsn = std::env::var("TB_LIVE_READONLY_DATABASE_URL")
        .expect("TB_LIVE_READONLY_DATABASE_URL fehlt");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET default_transaction_read_only = on")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&dsn)
        .await
        .expect("connect live db read-only");

    let (status, json) =
        response_json(pool, "/twitch/api/v2/public/streamer-comparison?days=30").await;

    assert_eq!(status, StatusCode::OK);
    assert!(json["streamers"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty()));
    assert!(
        json["network"]["streamerCount"]
            .as_u64()
            .unwrap_or_default()
            > 0
    );
}
