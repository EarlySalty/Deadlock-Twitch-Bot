//! `tb_twitch_user_id` muss die Tabellen finden, die im `search_path` der
//! aufrufenden Verbindung liegen — nicht nur die in `public`.
//!
//! Der Grund ist kein Selbstzweck: die hermetischen Fixtures der Crates legen
//! pro Test ein eigenes Schema an und setzen `search_path` darauf (siehe
//! `tb-monitoring/tests/support/mod.rs`). Wäre die Funktion fest an `public`
//! gebunden, gäbe sie dort für jeden Login `NULL` zurück — jede auf sie
//! umgestellte Query liefe im Test still leer, ohne dass ein Test rot wird.

use std::str::FromStr;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;

/// Die Migration ist die einzige Quelle der Funktionsdefinition; die Fixtures
/// der anderen Crates binden dieselbe Datei ein.
const LOOKUP_MIGRATION: &str =
    include_str!("../../../migrations/20260802120000_tb_twitch_user_id_search_path.sql");

fn test_dsn() -> Option<String> {
    std::env::var("TB_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("TEST_DATABASE_URL"))
        .ok()
}

/// Frisches Schema mit ausschließlich der Lookup-Funktion — bewusst ohne
/// Migrationslauf, damit der Test denselben schlanken Aufbau prüft, den die
/// Fixtures der anderen Crates benutzen.
async fn schema_pool(schema: &str) -> Option<PgPool> {
    let dsn = test_dsn()?;
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(10))
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
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(opts)
        .await
        .expect("connect");

    sqlx::raw_sql(LOOKUP_MIGRATION)
        .execute(&pool)
        .await
        .expect("Lookup-Funktion anlegen");
    Some(pool)
}

async fn aufloesen(pool: &PgPool, login: &str) -> Option<String> {
    sqlx::query_scalar::<_, Option<String>>("SELECT tb_twitch_user_id($1)")
        .bind(login)
        .fetch_one(pool)
        .await
        .expect("lookup")
}

#[tokio::test]
async fn loest_login_aus_dem_eigenen_schema_auf() {
    let Some(pool) = schema_pool("tb_test_lookup_identities").await else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };
    sqlx::query(
        "CREATE TABLE twitch_streamer_identities (
            twitch_login TEXT,
            twitch_user_id TEXT
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO twitch_streamer_identities VALUES ('coolysdl', '520300019')")
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(
        aufloesen(&pool, "coolysdl").await.as_deref(),
        Some("520300019"),
        "Identität liegt im Test-Schema und muss gefunden werden"
    );
    assert_eq!(
        aufloesen(&pool, "CoolysDL").await.as_deref(),
        Some("520300019"),
        "Twitch-Logins sind case-insensitiv"
    );
    assert_eq!(
        aufloesen(&pool, "niemand").await,
        None,
        "unbekannter Login darf nicht raten"
    );
}

#[tokio::test]
async fn faellt_auf_die_alias_historie_zurueck() {
    let Some(pool) = schema_pool("tb_test_lookup_aliases").await else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };
    sqlx::query(
        "CREATE TABLE twitch_login_aliases (
            twitch_user_id TEXT NOT NULL,
            login TEXT NOT NULL,
            is_current BOOLEAN NOT NULL DEFAULT FALSE
        )",
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

    assert_eq!(
        aufloesen(&pool, "derechtecoolys").await.as_deref(),
        Some("520300019"),
        "der alte Name muss weiter auf dieselbe ID zeigen"
    );
    assert_eq!(
        aufloesen(&pool, "recycelt").await,
        None,
        "ein von Twitch wieder freigegebener Name ist mehrdeutig — nicht raten"
    );
}

#[tokio::test]
async fn ueberspringt_fehlende_quelltabellen() {
    // Ein Fixture, das nur eine der drei Quelltabellen kennt, darf die Funktion
    // nicht mit "relation does not exist" sprengen.
    let Some(pool) = schema_pool("tb_test_lookup_leer").await else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };

    assert_eq!(
        aufloesen(&pool, "irgendwer").await,
        None,
        "ohne jede Quelltabelle bleibt das Ergebnis NULL statt Fehler"
    );
    assert_eq!(aufloesen(&pool, "  ").await, None, "leerer Login ist NULL");
}
