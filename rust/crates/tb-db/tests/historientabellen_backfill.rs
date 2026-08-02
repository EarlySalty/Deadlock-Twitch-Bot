//! Der Backfill der Kanal-ID in den Historientabellen.
//!
//! Geprüft wird nicht, dass Spalten existieren — das tut der
//! Schema-Snapshot —, sondern dass der Backfill die richtigen Zeilen füllt
//! und die richtigen offen lässt:
//!   * `twitch_stream_sessions` ist die Quelle und wird zuerst aufgelöst,
//!   * ihre Abnehmer erben die ID über `session_id`, nicht über den Namen,
//!   * ein von Twitch freigegebener und neu vergebener Name bleibt `NULL`,
//!   * Akteur-Rollen werden bewusst nicht angefasst.

use std::str::FromStr;
use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;

static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// Der Backfill-Teil der Migration. Er ist idempotent (`UPDATE ... WHERE
/// <id> IS NULL`), lässt sich also nach dem Seeden erneut anwenden — anders
/// käme man an Zeilen, die es zur Migrationszeit noch nicht gab, nicht heran.
const BACKFILL: &str =
    include_str!("../../../migrations/20260802140000_kanal_id_spalten_historientabellen.sql");

fn test_dsn() -> Option<String> {
    std::env::var("TB_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("TEST_DATABASE_URL"))
        .ok()
}

async fn migrated_pool(db_name: &str) -> Option<PgPool> {
    let dsn = test_dsn()?;
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&dsn)
        .await
        .expect("admin connect");
    sqlx::query(&format!("DROP DATABASE IF EXISTS {db_name} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE DATABASE {db_name}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;

    let opts = PgConnectOptions::from_str(&dsn)
        .expect("dsn parse")
        .database(db_name);
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(opts)
        .await
        .expect("connect");
    sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb")
        .execute(&pool)
        .await
        .ok();
    MIGRATOR.run(&pool).await.expect("run all migrations");
    Some(pool)
}

#[tokio::test]
async fn backfill_loest_kanaele_auf_und_laesst_mehrdeutiges_offen() {
    let Some(pool) = migrated_pool("tb_test_historien_backfill").await else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };

    sqlx::query(
        "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login)
         VALUES ('520300019', 'coolysdl')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_login_aliases (twitch_user_id, login, is_current) VALUES
            ('520300019', 'derechtecoolys', FALSE),
            ('520300019', 'coolysdl', TRUE),
            ('111', 'recycelt', FALSE),
            ('222', 'recycelt', FALSE)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Eine Session unter dem alten Namen und eine unter einem Namen, den
    // Twitch inzwischen an jemand anderen vergeben hat.
    let alte_session: i64 = sqlx::query_scalar(
        "INSERT INTO twitch_stream_sessions (streamer_login, started_at)
         VALUES ('derechtecoolys', NOW()) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let mehrdeutige_session: i64 = sqlx::query_scalar(
        "INSERT INTO twitch_stream_sessions (streamer_login, started_at)
         VALUES ('recycelt', NOW()) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Der Report hängt an der Session. Sein eigener streamer_login ist
    // absichtlich ein dritter, unbekannter Name: er darf die Auflösung nicht
    // beeinflussen, die session_id ist der verlässlichere Bezug.
    sqlx::query(
        "INSERT INTO twitch_stream_ai_reports (session_id, streamer_login, model)
         VALUES ($1, 'voellig_unbekannt', 'test')",
    )
    .bind(alte_session)
    .execute(&pool)
    .await
    .unwrap();

    // Akteur-Rolle: darf der Backfill nicht anfassen.
    sqlx::query(
        "INSERT INTO twitch_chatter_global_ban_applied (chatter_login, applied_at)
         VALUES ('coolysdl', NOW())",
    )
    .execute(&pool)
    .await
    .ok();

    sqlx::raw_sql(BACKFILL).execute(&pool).await.expect("Backfill erneut anwenden");

    let sessions: Vec<(i64, Option<String>)> = sqlx::query_as(
        "SELECT id, twitch_user_id FROM twitch_stream_sessions ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        sessions,
        vec![
            (alte_session, Some("520300019".to_string())),
            (mehrdeutige_session, None),
        ],
        "der frühere Name löst auf, der wiedervergebene bleibt offen statt geraten"
    );

    let report_id: Option<String> = sqlx::query_scalar(
        "SELECT twitch_user_id FROM twitch_stream_ai_reports WHERE session_id = $1",
    )
    .bind(alte_session)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        report_id.as_deref(),
        Some("520300019"),
        "der Report erbt die ID über die session_id, nicht über seinen eigenen Login"
    );
}

#[tokio::test]
async fn backfill_fasst_akteur_rollen_nicht_an() {
    let Some(pool) = migrated_pool("tb_test_historien_akteure").await else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };

    // Akteur-Spalten dürfen keine ID-Spalte aus dieser Migration bekommen —
    // ihre Identität gehört aus dem Event-Payload, nicht aus einer
    // Namensauflösung, die bei einem freigegebenen Namen die falsche Person
    // trifft.
    for (tabelle, spalte) in [
        ("twitch_chatter_global_ban_applied", "chatter_id"),
        ("twitch_viewer_presence_ticks", "viewer_id"),
    ] {
        let vorhanden: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM information_schema.columns
                             WHERE table_name = $1 AND column_name = $2)",
        )
        .bind(tabelle)
        .bind(spalte)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            !vorhanden,
            "{tabelle}.{spalte} darf hier noch nicht existieren — Akteur-Rollen sind eine eigene Runde"
        );
    }
}
