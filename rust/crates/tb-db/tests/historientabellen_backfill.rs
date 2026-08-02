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

/// Der Backfill von `twitch_raid_retention` übernimmt die Broadcaster-IDs
/// über den Join auf `twitch_raid_history` statt über die Namensauflösung —
/// nachdem der Namensweg auf Prod den Fremdschlüssel
/// `twitch_raid_retention_raid_history_ref_fkey` auslöste und den Bot-Startup
/// abbrach. (Die genaue Ursache dort ist nicht abschließend geklärt, siehe
/// Migrationskopf; die Prod-Daten widerlegen die naheliegende Erklärung
/// "verwaiste Zeilen" — davon gibt es null.)
///
/// Dieser Test hält fest, was der Join leisten muss: Zeilen mit History-Zeile
/// erben deren IDs, und eine Zeile ohne History-Partner bringt den Backfill
/// weder zum Absturz noch zu einer geratenen ID.
#[tokio::test]
async fn backfill_stolpert_nicht_ueber_verwaiste_raid_retention_zeilen() {
    let Some(pool) = migrated_pool("tb_test_raid_retention_fk").await else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };

    // Der FK weicht kurz, damit überhaupt eine verwaiste Zeile entstehen kann,
    // und kommt als NOT VALID zurück. Das ist ein konstruierter Fall, kein
    // Abbild von Prod — dort gibt es null verwaiste Zeilen (siehe Doc-Kommentar
    // oben). Geprüft wird hier, dass der Join auch damit umgehen kann.
    sqlx::query("ALTER TABLE twitch_raid_retention DROP CONSTRAINT twitch_raid_retention_raid_history_ref_fkey")
        .execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO twitch_raid_history
            (id, executed_at, from_broadcaster_login, from_broadcaster_id,
             to_broadcaster_login, to_broadcaster_id)
         VALUES (9001, NOW(), 'derechtecoolys', '520300019', 'ziel', '777')",
    ).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO twitch_raid_retention
            (raid_id, executed_at, from_broadcaster_login, to_broadcaster_login, viewer_count_sent)
         SELECT 9001, executed_at, 'derechtecoolys', 'ziel', 5
           FROM twitch_raid_history WHERE id = 9001",
    ).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO twitch_raid_retention
            (raid_id, executed_at, from_broadcaster_login, to_broadcaster_login, viewer_count_sent)
         VALUES (9999, NOW(), 'verwaist', 'auchverwaist', 3)",
    ).execute(&pool).await.unwrap();
    sqlx::query(
        "ALTER TABLE twitch_raid_retention ADD CONSTRAINT twitch_raid_retention_raid_history_ref_fkey
           FOREIGN KEY (raid_id, executed_at) REFERENCES twitch_raid_history(id, executed_at)
           ON DELETE CASCADE NOT VALID",
    ).execute(&pool).await.unwrap();

    sqlx::raw_sql(BACKFILL)
        .execute(&pool)
        .await
        .expect("Backfill darf an der verwaisten Zeile nicht scheitern");

    let zeilen: Vec<(i64, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT raid_id, from_broadcaster_id, to_broadcaster_id
           FROM twitch_raid_retention ORDER BY raid_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        zeilen,
        vec![
            (9001, Some("520300019".to_string()), Some("777".to_string())),
            (9999, None, None),
        ],
        "die Zeile mit History erbt beide IDs, die verwaiste bleibt unberührt"
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
