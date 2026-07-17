//! Env-gated Integrationstest für den Write-Retry-Wrapper (`tb_db::run_transaction`).
//! Läuft nur mit `TB_TEST_DATABASE_URL` (NIE gegen Prod). Ohne diese Variable:
//! sauberer Skip, damit `cargo test` in der CI grün bleibt.
//!
//! Verifiziert gegen eine echte Postgres-Instanz:
//!   1. Erfolgreiche Transaktion committet (Wert persistiert).
//!   2. Retrybare SQLSTATEs (40001) werden bis zum Erfolg erneut versucht.
//!   3. Nicht-retrybare Fehler werden ohne Retry sofort weitergereicht.
//!   4. Erschöpfte Versuche reichen den letzten Fehler durch.
//!
//! Hinweis: Der sqlx-Pool gibt pro Query ggf. eine andere Verbindung aus, daher
//! nutzen die Tests reale (nicht TEMP) Tabellen als sitzungsunabhängigen Zähler
//! und räumen am Ende selbst auf. So bleibt der Versuchszähler über alle
//! Retry-Iterationen hinweg konsistent sichtbar.

use std::time::Duration;

use sqlx::Row;
use tb_config::DbConfig;
use tb_db::{run_transaction, DbError, IsolationLevel, RetryPolicy};

fn test_dsn() -> Option<String> {
    std::env::var("TB_TEST_DATABASE_URL").ok()
}

/// Schnelle Politik fürs Testen: 3 Versuche, vernachlässigbarer Backoff.
fn fast_policy() -> RetryPolicy {
    RetryPolicy {
        max_attempts: 3,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(2),
    }
}

async fn connect(dsn: &str) -> sqlx::PgPool {
    let cfg = DbConfig {
        dsn: dsn.to_string(),
        pool_max: 2,
        acquire_timeout: Duration::from_secs(10),
        connect_timeout: Duration::from_secs(5),
    };
    tb_db::connect(&cfg).await.expect("connect test db")
}

/// Legt eine eindeutig benannte Zähltabelle an und gibt ihren Namen zurück.
async fn fresh_counter(pool: &sqlx::PgPool, suffix: &str) -> String {
    let table = format!("retry_test_{suffix}");
    sqlx::query(&format!("DROP TABLE IF EXISTS {table}"))
        .execute(pool)
        .await
        .expect("drop");
    sqlx::query(&format!(
        "CREATE TABLE {table} (id integer PRIMARY KEY, attempts integer NOT NULL DEFAULT 0)"
    ))
    .execute(pool)
    .await
    .expect("create");
    sqlx::query(&format!("INSERT INTO {table} (id, attempts) VALUES (1, 0)"))
        .execute(pool)
        .await
        .expect("seed");
    table
}

async fn drop_table(pool: &sqlx::PgPool, table: &str) {
    let _ = sqlx::query(&format!("DROP TABLE IF EXISTS {table}"))
        .execute(pool)
        .await;
}

async fn attempts(pool: &sqlx::PgPool, table: &str) -> i32 {
    sqlx::query(&format!("SELECT attempts FROM {table} WHERE id = 1"))
        .fetch_one(pool)
        .await
        .expect("read attempts")
        .get("attempts")
}

#[tokio::test]
async fn commits_successful_transaction() {
    let Some(dsn) = test_dsn() else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt.");
        return;
    };
    let pool = connect(&dsn).await;
    let table = fresh_counter(&pool, "commit").await;
    let t = table.clone();

    let written: i32 = run_transaction(&pool, IsolationLevel::ReadCommitted, fast_policy(), |tx| {
        let t = t.clone();
        Box::pin(async move {
            sqlx::query(&format!("UPDATE {t} SET attempts = 42 WHERE id = 1"))
                .execute(&mut **tx)
                .await?;
            Ok::<_, DbError>(42)
        })
    })
    .await
    .expect("transaction commits");
    assert_eq!(written, 42);
    assert_eq!(
        attempts(&pool, &table).await,
        42,
        "committeter Wert persistiert"
    );

    drop_table(&pool, &table).await;
}

#[tokio::test]
async fn retries_serialization_failure_until_success() {
    let Some(dsn) = test_dsn() else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt.");
        return;
    };
    let pool = connect(&dsn).await;
    // Separater Pool für den Auto-Commit-Zähler, damit er nicht mit der gehaltenen
    // Transaktionsverbindung um denselben Pool-Slot konkurriert.
    let counter = connect(&dsn).await;
    let table = fresh_counter(&pool, "serialize").await;
    let t = table.clone();

    // Jeder Versuch erhöht den Zähler in einer SEPARATEN Auto-Commit-Query (also
    // außerhalb der zurückgerollten Transaktion), raised aber bei den ersten zwei
    // Versuchen einen echten 40001. Erst der dritte Versuch committet sauber.
    let pool_for_op = counter;
    let result = run_transaction(&pool, IsolationLevel::ReadCommitted, fast_policy(), |tx| {
        let t = t.clone();
        let counter_pool = pool_for_op.clone();
        Box::pin(async move {
            // Zähler außerhalb der TX hochzählen (überlebt den Rollback).
            let attempt: i32 = sqlx::query(&format!(
                "UPDATE {t} SET attempts = attempts + 1 WHERE id = 1 RETURNING attempts"
            ))
            .fetch_one(&counter_pool)
            .await?
            .get("attempts");
            if attempt < 3 {
                sqlx::query("DO $$ BEGIN RAISE EXCEPTION 'forced' USING ERRCODE = '40001'; END $$")
                    .execute(&mut **tx)
                    .await?;
            }
            Ok::<_, DbError>(attempt)
        })
    })
    .await
    .expect("retries until commit");
    assert_eq!(result, 3, "muss genau im dritten Versuch erfolgreich sein");
    assert_eq!(
        attempts(&pool, &table).await,
        3,
        "genau drei Versuche gezählt"
    );

    drop_table(&pool, &table).await;
}

#[tokio::test]
async fn non_retryable_error_propagates_immediately() {
    let Some(dsn) = test_dsn() else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt.");
        return;
    };
    let pool = connect(&dsn).await;
    let counter = connect(&dsn).await;
    let table = fresh_counter(&pool, "nonretry").await;
    let t = table.clone();

    let pool_for_op = counter;
    let err = run_transaction(&pool, IsolationLevel::ReadCommitted, fast_policy(), |tx| {
        let t = t.clone();
        let counter_pool = pool_for_op.clone();
        Box::pin(async move {
            sqlx::query(&format!(
                "UPDATE {t} SET attempts = attempts + 1 WHERE id = 1"
            ))
            .execute(&counter_pool)
            .await?;
            // 23505 = unique_violation → NICHT retrybar.
            sqlx::query("DO $$ BEGIN RAISE EXCEPTION 'nope' USING ERRCODE = '23505'; END $$")
                .execute(&mut **tx)
                .await?;
            Ok::<_, DbError>(())
        })
    })
    .await;
    assert!(
        matches!(err, Err(DbError::Sqlx(_))),
        "Fehler muss propagieren"
    );
    assert_eq!(
        attempts(&pool, &table).await,
        1,
        "darf bei nicht-retrybarem Fehler nicht erneut versuchen"
    );

    drop_table(&pool, &table).await;
}

#[tokio::test]
async fn exhausted_retries_propagate_last_error() {
    let Some(dsn) = test_dsn() else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt.");
        return;
    };
    let pool = connect(&dsn).await;
    let counter = connect(&dsn).await;
    let table = fresh_counter(&pool, "exhaust").await;
    let t = table.clone();

    let pool_for_op = counter;
    let err = run_transaction(&pool, IsolationLevel::ReadCommitted, fast_policy(), |tx| {
        let t = t.clone();
        let counter_pool = pool_for_op.clone();
        Box::pin(async move {
            sqlx::query(&format!(
                "UPDATE {t} SET attempts = attempts + 1 WHERE id = 1"
            ))
            .execute(&counter_pool)
            .await?;
            // Immer 40001 → erschöpft alle Versuche.
            sqlx::query("DO $$ BEGIN RAISE EXCEPTION 'always' USING ERRCODE = '40001'; END $$")
                .execute(&mut **tx)
                .await?;
            Ok::<_, DbError>(())
        })
    })
    .await;
    assert!(
        matches!(err, Err(DbError::Sqlx(_))),
        "letzter Fehler muss propagieren"
    );
    assert_eq!(
        attempts(&pool, &table).await,
        3,
        "genau max_attempts Versuche"
    );

    drop_table(&pool, &table).await;
}
