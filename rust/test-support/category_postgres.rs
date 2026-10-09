#[path = "database.rs"]
mod configuration;
#[path = "postgres.rs"]
mod local;

pub struct TestPostgres {
    pub pool: sqlx::PgPool,
    _local: Option<local::TestPostgres>,
}
impl TestPostgres {
    pub async fn start() -> Self {
        use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
        use std::str::FromStr;
        let Some(dsn) = configuration::database_url() else {
            let local = local::TestPostgres::start().await;
            return Self {
                pool: local.pool.clone(),
                _local: Some(local),
            };
        };
        let options = PgConnectOptions::from_str(&dsn).expect("Test-DSN");
        assert_eq!(options.get_host(), "127.0.0.1");
        assert_eq!(options.get_port(), 33100);
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await
            .expect("Freigegebener Testcontainer");
        let name = format!("category_storage_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let pool = PgPoolOptions::new()
            .max_connections(6)
            .connect_with(options.database(&name))
            .await
            .unwrap();
        Self { pool, _local: None }
    }
}

pub async fn storage_schema(pool: &sqlx::PgPool) {
    sqlx::raw_sql("DO $$ BEGIN IF NOT EXISTS(SELECT FROM pg_roles WHERE rolname='twitchbot') THEN CREATE ROLE twitchbot NOLOGIN; END IF; IF NOT EXISTS(SELECT FROM pg_roles WHERE rolname='twitchdash') THEN CREATE ROLE twitchdash NOLOGIN; END IF; END $$;")
        .execute(pool).await.unwrap();
    sqlx::raw_sql("CREATE TABLE IF NOT EXISTS twitch_partners(twitch_user_id text PRIMARY KEY,status text NOT NULL DEFAULT 'active'); CREATE OR REPLACE VIEW twitch_streamers_partner_state AS SELECT twitch_user_id,1 AS is_partner FROM twitch_partners WHERE status='active';")
        .execute(pool).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../migrations/20261009110000_category_storage.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
}
