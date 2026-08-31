//! Datenmigration für irreversible Dashboard-Session-Lookups.
//!
//! Ohne `TB_TEST_DATABASE_URL` wird der PostgreSQL-Test laut übersprungen.

use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;

fn test_dsn() -> Option<String> {
    std::env::var("TB_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("TEST_DATABASE_URL"))
        .ok()
}

fn lookup_key(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}

#[tokio::test]
async fn migration_hashes_aktive_session_ohne_sie_zu_invalidieren() {
    let Some(dsn) = test_dsn() else {
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("Test-PostgreSQL verbinden");
    let schema = format!(
        "session_hash_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Systemzeit")
            .as_nanos()
    );
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&pool)
        .await
        .expect("Testschema anlegen");
    sqlx::query(&format!(
        "CREATE TABLE {schema}.dashboard_sessions (\
             session_id TEXT PRIMARY KEY, session_type TEXT NOT NULL, \
             payload_enc BYTEA NOT NULL, created_at DOUBLE PRECISION NOT NULL, \
             expires_at DOUBLE PRECISION NOT NULL)"
    ))
    .execute(&pool)
    .await
    .expect("Session-Tabelle anlegen");

    let raw_session = "aktive-session-🔥";
    let rate_id = "rl:60:iphash:123:abcd";
    sqlx::query(&format!(
        "INSERT INTO {schema}.dashboard_sessions \
         (session_id, session_type, payload_enc, created_at, expires_at) \
         VALUES ($1, 'twitch', $2, 1, 9999999999), \
                ($3, 'rate_limit:dashboard_auth', $2, 1, 9999999999)"
    ))
    .bind(raw_session)
    .bind(b"encrypted-payload".as_slice())
    .bind(rate_id)
    .execute(&pool)
    .await
    .expect("Testdaten schreiben");

    let migration =
        include_str!("../../../migrations/20260831090000_dashboard_session_ids_sha256.sql")
            .replace(
                "public.dashboard_sessions",
                &format!("{schema}.dashboard_sessions"),
            );
    sqlx::raw_sql(&migration)
        .execute(&pool)
        .await
        .expect("Hash-Migration ausführen");

    let stored: Vec<String> = sqlx::query_scalar(&format!(
        "SELECT session_id FROM {schema}.dashboard_sessions ORDER BY session_type"
    ))
    .fetch_all(&pool)
    .await
    .expect("migrierte IDs lesen");
    assert!(stored.contains(&lookup_key(raw_session)));
    assert!(!stored.iter().any(|value| value == raw_session));
    assert!(stored.iter().any(|value| value == rate_id));

    let active_payload: Vec<u8> = sqlx::query_scalar(&format!(
        "SELECT payload_enc FROM {schema}.dashboard_sessions WHERE session_id = $1"
    ))
    .bind(lookup_key(raw_session))
    .fetch_one(&pool)
    .await
    .expect("aktive Session bleibt über Hash auffindbar");
    assert_eq!(active_payload, b"encrypted-payload");

    let invalid = sqlx::query(&format!(
        "INSERT INTO {schema}.dashboard_sessions \
         (session_id, session_type, payload_enc, created_at, expires_at) \
         VALUES ('neuer-rohwert', 'twitch', 'x', 1, 2)"
    ))
    .execute(&pool)
    .await;
    assert!(invalid.is_err(), "Constraint muss neue Rohwerte abweisen");

    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&pool)
        .await
        .expect("Testschema entfernen");
}
