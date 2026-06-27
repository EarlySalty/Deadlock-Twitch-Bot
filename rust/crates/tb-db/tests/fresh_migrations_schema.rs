//! Frische Migrationen gegen eine leere Wegwerf-DB.
//! Ohne `TEST_DATABASE_URL` wird der Test laut uebersprungen.

use std::time::Duration;

use sqlx::postgres::PgPoolOptions;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

fn test_dsn() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

async fn column_type(pool: &sqlx::PgPool, table: &str, column: &str) -> String {
    sqlx::query_scalar(
        "SELECT data_type
           FROM information_schema.columns
          WHERE table_schema = 'public'
            AND table_name = $1
            AND column_name = $2",
    )
    .bind(table)
    .bind(column)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|err| panic!("column type for {table}.{column}: {err}"))
}

#[tokio::test]
async fn fresh_migrations_apply_expected_analytics_schema_types() {
    let dsn = match test_dsn() {
        Some(dsn) => dsn,
        None => {
            eprintln!("SKIP: TEST_DATABASE_URL nicht gesetzt");
            return;
        }
    };

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&dsn)
        .await
        .expect("connect fresh test db");

    sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb")
        .execute(&pool)
        .await
        .expect("create timescaledb extension");

    MIGRATOR.run(&pool).await.expect("run all migrations");

    for (table, column, expected) in [
        ("twitch_stream_sessions", "id", "bigint"),
        (
            "twitch_stream_sessions",
            "avg_viewers",
            "double precision",
        ),
        ("twitch_session_viewers", "session_id", "bigint"),
        (
            "twitch_session_viewers",
            "ts_utc",
            "timestamp with time zone",
        ),
        ("twitch_chat_messages", "session_id", "bigint"),
        (
            "twitch_chat_messages",
            "message_ts",
            "timestamp with time zone",
        ),
        ("twitch_raid_retention", "target_session_id", "bigint"),
    ] {
        let actual = column_type(&pool, table, column).await;
        assert_eq!(actual, expected, "{table}.{column}");
    }

    let stream_session_sequence_type: String = sqlx::query_scalar(
        "SELECT format_type(seqtypid, NULL)
           FROM pg_sequence
          WHERE seqrelid = 'public.twitch_stream_sessions_id_seq'::regclass",
    )
    .fetch_one(&pool)
    .await
    .expect("twitch_stream_sessions_id_seq type");
    assert_eq!(stream_session_sequence_type, "bigint");
}
