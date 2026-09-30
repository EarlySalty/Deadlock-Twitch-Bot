use std::{str::FromStr, time::Duration};

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use tb_config::DbConfig;

#[path = "../../../test-support/database.rs"]
mod test_database;

#[tokio::test]
async fn read_only_settings_apply_to_multiple_connections_and_deny_persistent_writes() {
    let Some(dsn) = test_database::database_url() else {
        assert!(
            !test_database::required(),
            "PostgreSQL test config is required"
        );
        return;
    };
    let admin_options = PgConnectOptions::from_str(&dsn).expect("parse configured test DSN");
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(admin_options.clone())
        .await
        .expect("connect configured test database");
    let table = format!(
        "readonly_probe_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let create = format!("CREATE TABLE {table} (value integer NOT NULL)");
    let insert = format!("INSERT INTO {table} (value) VALUES (1)");
    sqlx::query(sqlx::AssertSqlSafe(create))
        .execute(&admin)
        .await
        .expect("create persistent probe table");

    let cfg = DbConfig {
        dsn,
        pool_max: 3,
        acquire_timeout: Duration::from_secs(10),
        connect_timeout: Duration::from_secs(10),
    };
    let readonly = tb_db::pool::connect_readonly(&cfg)
        .await
        .expect("open read-only pool");
    let mut connections = Vec::new();
    for _ in 0..3 {
        connections.push(readonly.acquire().await.expect("hold pool connection"));
    }
    for connection in &mut connections {
        let settings: (bool, bool) = sqlx::query_as(
            "SELECT current_setting('default_transaction_read_only')::boolean, current_setting('statement_timeout')::interval = interval '20 seconds'",
        )
        .fetch_one(&mut **connection)
        .await
        .expect("read connection settings");
        assert_eq!(settings, (true, true));
    }
    let error = sqlx::query(sqlx::AssertSqlSafe(insert))
        .execute(&mut *connections[0])
        .await
        .expect_err("read-only connection must reject persistent table writes");
    assert_eq!(
        error
            .as_database_error()
            .and_then(|db| db.code())
            .as_deref(),
        Some("25006")
    );

    drop(connections);
    readonly.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP TABLE {table}")))
        .execute(&admin)
        .await
        .expect("drop persistent probe table");
    admin.close().await;
}
