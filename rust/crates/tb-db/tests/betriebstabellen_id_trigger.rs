//! Der Übergangstrigger, der die stabile Twitch-ID aus dem Login auflöst,
//! solange die Schreibpfade sie noch nicht selbst mitgeben.

use std::str::FromStr;
use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;

static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

fn test_dsn() -> Option<String> {
    std::env::var("TB_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("TEST_DATABASE_URL"))
        .ok()
}

/// Frische Datenbank mit allen Migrationen. Ein eigenes Schema reicht nicht:
/// ältere Migrationen greifen fest auf `public` zu und scheitern sonst.
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
async fn trigger_traegt_die_twitch_user_id_beim_schreiben_nach() {
    let Some(pool) = migrated_pool("tb_test_id_trigger_insert").await else {
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

    sqlx::query("INSERT INTO twitch_engagement_settings (channel_login) VALUES ('CoolysDL')")
        .execute(&pool)
        .await
        .unwrap();
    // Unbekannter Login: NULL ist die ehrliche Antwort, nicht irgendeine ID.
    sqlx::query("INSERT INTO twitch_engagement_settings (channel_login) VALUES ('niemand')")
        .execute(&pool)
        .await
        .unwrap();

    let zeilen: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT channel_login, channel_user_id
           FROM twitch_engagement_settings ORDER BY channel_login",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(
        zeilen,
        vec![
            ("CoolysDL".to_string(), Some("520300019".to_string())),
            ("niemand".to_string(), None),
        ]
    );
}

#[tokio::test]
async fn trigger_ueberschreibt_eine_bereits_gesetzte_id_nicht() {
    let Some(pool) = migrated_pool("tb_test_id_trigger_keeps").await else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };
    // Die Identitätstabelle kennt den Login unter einer anderen ID — das ist
    // genau der Zustand kurz nach einer Umbenennung.
    sqlx::query(
        "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login)
         VALUES ('999', 'coolysdl')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_engagement_settings (channel_login, channel_user_id)
         VALUES ('coolysdl', '520300019')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Auch ein Login-Update darf die vom Rename gesetzte ID nicht kippen.
    sqlx::query("UPDATE twitch_engagement_settings SET channel_login = 'coolysdl2'")
        .execute(&pool)
        .await
        .unwrap();

    let id: Option<String> =
        sqlx::query_scalar("SELECT channel_user_id FROM twitch_engagement_settings")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(id.as_deref(), Some("520300019"));
}

#[tokio::test]
async fn lookup_loest_login_zur_stabilen_id_auf() {
    let Some(pool) = migrated_pool("tb_test_id_lookup").await else {
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
    // Ein früherer Name desselben Kanals bleibt auflösbar …
    sqlx::query(
        "INSERT INTO twitch_login_aliases (twitch_user_id, login, is_current)
         VALUES ('520300019', 'derechtecoolys', FALSE)",
    )
    .execute(&pool)
    .await
    .unwrap();
    // … ein von Twitch neu vergebener dagegen nicht: zwei IDs, keine Wahl.
    for statement in [
        "INSERT INTO twitch_login_aliases (twitch_user_id, login, is_current)
         VALUES ('111', 'recycelt', FALSE)",
        "INSERT INTO twitch_login_aliases (twitch_user_id, login, is_current)
         VALUES ('222', 'recycelt', FALSE)",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }

    let treffer: (Option<String>, Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT tb_twitch_user_id('CoolysDL'), tb_twitch_user_id('derechtecoolys'),
                tb_twitch_user_id('recycelt'), tb_twitch_user_id('  ')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(
        treffer,
        (
            Some("520300019".to_string()),
            Some("520300019".to_string()),
            None,
            None
        )
    );
}
