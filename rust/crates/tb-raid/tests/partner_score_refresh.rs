//! Hermetischer Integrationstest der Partner-Raid-Score Refresh-Pipeline (P1.9).
//!
//! Beweist die geschlossene Lücke: `PartnerScoreRefresher::refresh_all` schreibt
//! `twitch_partner_raid_scores`-Zeilen mit voranschreitendem `last_computed_at`
//! und korrektem `final_score`/`today_received_raids` für einen LIVE- und einen
//! OFFLINE-Partner.

use std::str::FromStr;

use chrono::{TimeZone, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_raid::PartnerScoreRefresher;

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
    create_schema(&pool).await;
    pool
}

async fn create_schema(pool: &PgPool) {
    // Score-Cache (Prod-Schema: Flags INTEGER, TEXT-Timestamps — wie score_store).
    sqlx::query(
        r#"
        CREATE TABLE twitch_partner_raid_scores (
            twitch_user_id                  TEXT PRIMARY KEY,
            twitch_login                    TEXT NOT NULL,
            avg_duration_sec                INTEGER NOT NULL DEFAULT 0,
            time_pattern_score_base         DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            received_successful_raids_total INTEGER NOT NULL DEFAULT 0,
            is_new_partner_preferred        INTEGER NOT NULL DEFAULT 0,
            new_partner_multiplier          DOUBLE PRECISION NOT NULL DEFAULT 1.0,
            raid_boost_multiplier           DOUBLE PRECISION NOT NULL DEFAULT 1.0,
            is_live                         INTEGER NOT NULL DEFAULT 0,
            current_started_at              TEXT,
            current_uptime_sec              INTEGER NOT NULL DEFAULT 0,
            duration_score                  DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            time_pattern_score              DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            base_score                      DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            final_score                     DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            today_received_raids            INTEGER NOT NULL DEFAULT 0,
            last_computed_at                TEXT NOT NULL,
            readiness_score                 DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            fairness_score                  DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            internal_sent_raids_30d         INTEGER NOT NULL DEFAULT 0,
            internal_received_raids_7d      INTEGER NOT NULL DEFAULT 0,
            internal_received_raids_30d     INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(pool)
    .await
    .unwrap();

    // Partner-State.
    sqlx::query(
        r#"
        CREATE TABLE twitch_streamers_partner_state (
            twitch_user_id    TEXT PRIMARY KEY,
            twitch_login      TEXT NOT NULL,
            is_partner_active INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(pool)
    .await
    .unwrap();

    // Sessions (clean-SQL: TIMESTAMPTZ).
    sqlx::query(
        r#"
        CREATE TABLE twitch_stream_sessions (
            id               BIGSERIAL PRIMARY KEY,
            streamer_login   TEXT NOT NULL,
            started_at       TIMESTAMPTZ,
            duration_seconds BIGINT
        )
        "#,
    )
    .execute(pool)
    .await
    .unwrap();

    // Raid-History (clean-SQL: TIMESTAMPTZ, BOOLEAN-success).
    sqlx::query(
        r#"
        CREATE TABLE twitch_raid_history (
            id                  BIGSERIAL PRIMARY KEY,
            from_broadcaster_id TEXT,
            to_broadcaster_id   TEXT,
            executed_at         TIMESTAMPTZ,
            success             BOOLEAN
        )
        "#,
    )
    .execute(pool)
    .await
    .unwrap();

    // Live-State (clean-SQL: TIMESTAMPTZ).
    sqlx::query(
        r#"
        CREATE TABLE twitch_live_state (
            twitch_user_id  TEXT PRIMARY KEY,
            is_live         INTEGER NOT NULL DEFAULT 0,
            last_started_at TIMESTAMPTZ
        )
        "#,
    )
    .execute(pool)
    .await
    .unwrap();

    // Plans (vereinfachter Boost-Pfad).
    sqlx::query(
        r#"
        CREATE TABLE streamer_plans (
            twitch_user_id     TEXT PRIMARY KEY,
            raid_boost_enabled INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn refresh_all_schreibt_live_und_offline_partner() {
    let pool = pool_or_skip!("p1_9_refresh_all");

    // now = 21.6.2026 12:00 UTC (= 14:00 Berlin).
    let now = Utc.with_ymd_and_hms(2026, 6, 21, 12, 0, 0).unwrap();

    // Zwei aktive Partner: einer live, einer offline.
    sqlx::query(
        "INSERT INTO twitch_streamers_partner_state (twitch_user_id, twitch_login, is_partner_active) \
         VALUES ('uid_live', 'LivePartner', 1), ('uid_off', 'OfflinePartner', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Live-Partner: live seit 1h.
    let started = now - chrono::Duration::seconds(3600);
    sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, is_live, last_started_at) VALUES ('uid_live', 1, $1)")
        .bind(started)
        .execute(&pool)
        .await
        .unwrap();
    // Offline-Partner: nicht live.
    sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, is_live, last_started_at) VALUES ('uid_off', 0, NULL)")
        .execute(&pool)
        .await
        .unwrap();

    // Live-Partner: 3 zuverlässige Sessions à 2h → avg 7200, duration_score 0.5.
    for i in 1..=3 {
        sqlx::query("INSERT INTO twitch_stream_sessions (streamer_login, started_at, duration_seconds) VALUES ('livepartner', $1, 7200)")
            .bind(now - chrono::Duration::days(i))
            .execute(&pool)
            .await
            .unwrap();
    }

    // Live-Partner: ein erfolgreicher Raid heute, einer vor 2 Tagen → today=1, total=2.
    for ts in [
        now - chrono::Duration::hours(2),
        now - chrono::Duration::days(2),
    ] {
        sqlx::query("INSERT INTO twitch_raid_history (from_broadcaster_id, to_broadcaster_id, executed_at, success) VALUES ('someone', 'uid_live', $1, TRUE)")
            .bind(ts)
            .execute(&pool)
            .await
            .unwrap();
    }

    let refresher = PartnerScoreRefresher::new(pool.clone());
    let written = refresher.refresh_all(now).await.unwrap();
    assert_eq!(written, 2, "beide aktiven Partner geschrieben");

    // Live-Partner-Zeile prüfen.
    let (is_live, final_score, today, last_computed, uptime, dur): (i32, f64, i32, String, i32, f64) =
        sqlx::query_as(
            "SELECT is_live, final_score, today_received_raids, last_computed_at, current_uptime_sec, duration_score \
             FROM twitch_partner_raid_scores WHERE twitch_user_id = 'uid_live'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(is_live, 1);
    assert_eq!(today, 1, "ein Raid heute (Berlin-Datum)");
    assert_eq!(uptime, 3600);
    assert!(
        (dur - 0.5).abs() < 1e-9,
        "duration_score=0.5 bei halber avg-Zeit"
    );
    assert!(final_score > 0.0, "final_score berechnet (nicht 0)");
    assert_eq!(last_computed, "2026-06-21T12:00:00+00:00");

    // Offline-Partner-Zeile prüfen.
    let (off_live, off_final, off_today): (i32, f64, i32) = sqlx::query_as(
        "SELECT is_live, final_score, today_received_raids FROM twitch_partner_raid_scores WHERE twitch_user_id = 'uid_off'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(off_live, 0);
    assert_eq!(off_today, 0);
    // Offline neuer Partner: fairness(0,0,0,0)=0.75, base=0.5875, *1.25 new-mult.
    let expected_final = ((0.5875_f64 * 1.25) * 1e6).round() / 1e6;
    assert!(
        (off_final - expected_final).abs() < 1e-6,
        "offline final_score Formel"
    );

    // last_computed_at schreitet beim zweiten Refresh voran.
    let later = now + chrono::Duration::minutes(5);
    refresher.refresh_all(later).await.unwrap();
    let (lc2,): (String,) = sqlx::query_as(
        "SELECT last_computed_at FROM twitch_partner_raid_scores WHERE twitch_user_id = 'uid_live'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        lc2, "2026-06-21T12:05:00+00:00",
        "last_computed_at advances"
    );
    assert_ne!(lc2, last_computed);
}
