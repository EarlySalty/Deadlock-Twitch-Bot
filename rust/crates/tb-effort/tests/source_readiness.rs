use chrono::{DateTime, Duration, Utc};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::str::FromStr;
use tb_config::challenges::Challenges;
use tb_effort::Engine;

#[path = "../../../test-support/database.rs"]
mod test_database;

fn idle_helix() -> tb_transport_twitch::HelixClient {
    let mut config = tb_transport_twitch::HelixConfig::new("fixture", "fixture");
    config.token_url = "http://127.0.0.1:1/token".into();
    config.helix_base = "http://127.0.0.1:1".into();
    tb_transport_twitch::HelixClient::new(config).unwrap()
}

async fn fixture() -> (PgPool, PgPool, String) {
    let dsn = test_database::database_url().expect("isolated test database must be configured");
    let options = PgConnectOptions::from_str(&dsn).unwrap();
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options.clone())
        .await
        .unwrap();
    let name = format!(
        "tb_effort_source_{}_{}",
        std::process::id(),
        Utc::now().timestamp_subsec_nanos()
    );
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(16)
        .connect_with(options.database(&name))
        .await
        .unwrap();
    sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb")
        .execute(&pool)
        .await
        .unwrap();
    tb_db::run_migrations(&pool).await.unwrap();
    sqlx::query("UPDATE category_collector_config SET poll_seconds=300 WHERE singleton")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO category_collection_runs(snapshot_at,completed_at,streams,viewers,poll_seconds) SELECT at,at,0,0,300 FROM generate_series('2026-09-01T00:00:00Z'::timestamptz,'2026-11-02T00:00:00Z'::timestamptz,INTERVAL '5 minutes') AS at")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE SCHEMA activity; CREATE SCHEMA core; CREATE SCHEMA bot; CREATE SCHEMA steam;
        CREATE TABLE core.steam_links(discord_id bigint,steam_id text,verified boolean);
        CREATE TABLE activity.voice_session_log(id bigint PRIMARY KEY,user_id bigint,guild_id bigint,channel_id bigint,started_at timestamptz,ended_at timestamptz);
        CREATE TABLE steam.steam_tasks(id bigint PRIMARY KEY,type text,payload jsonb,status text,result jsonb,finished_at timestamptz);
        CREATE TABLE bot.twitch_invite_qualification_status(singleton boolean PRIMARY KEY DEFAULT TRUE CHECK(singleton),last_completed_at timestamptz NOT NULL,last_successful_at timestamptz,evaluation_interval_seconds integer NOT NULL CHECK(evaluation_interval_seconds BETWEEN 1 AND 86400),healthy boolean NOT NULL,CHECK(NOT healthy OR (last_successful_at IS NOT NULL AND last_successful_at=last_completed_at)));
        WITH completed AS (SELECT clock_timestamp() AS at)
        INSERT INTO bot.twitch_invite_qualification_status(singleton,last_completed_at,last_successful_at,evaluation_interval_seconds,healthy) SELECT TRUE,at,at,300,TRUE FROM completed;
        INSERT INTO twitch_partners(twitch_user_id,twitch_login,status) VALUES('101','alice','active'),('102','bob','active'),('103','inactive','inactive');")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("fixtures/qualified_twitch_invites.sql"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO category_collector_status(singleton,heartbeat_at,details) VALUES(TRUE,clock_timestamp(),'{}') ON CONFLICT(singleton) DO UPDATE SET heartbeat_at=EXCLUDED.heartbeat_at,details=EXCLUDED.details")
        .execute(&pool)
        .await
        .unwrap();
    (admin, pool, name)
}

async fn set_marker(
    pool: &PgPool,
    completed_at: DateTime<Utc>,
    successful_at: Option<DateTime<Utc>>,
    healthy: bool,
) {
    sqlx::query("INSERT INTO bot.twitch_invite_qualification_status(singleton,last_completed_at,last_successful_at,evaluation_interval_seconds,healthy) VALUES(TRUE,$1,$2,300,$3) ON CONFLICT(singleton) DO UPDATE SET last_completed_at=EXCLUDED.last_completed_at,last_successful_at=EXCLUDED.last_successful_at,evaluation_interval_seconds=EXCLUDED.evaluation_interval_seconds,healthy=EXCLUDED.healthy")
        .bind(completed_at)
        .bind(successful_at)
        .bind(healthy)
        .execute(pool)
        .await
        .unwrap();
}

async fn invites_healthy(pool: &PgPool) -> bool {
    sqlx::query_scalar("SELECT healthy FROM partner_effort_source_state WHERE source='invites'")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn invite_source_rejects_missing_stale_and_failed_markers_then_recovers_empty() {
    let (admin, pool, name) = fixture().await;
    let engine = Engine::new(
        pool.clone(),
        Challenges::default(),
        Some(pool.clone()),
        Some(idle_helix()),
    )
    .unwrap();

    sqlx::query("DELETE FROM bot.twitch_invite_qualification_status")
        .execute(&pool)
        .await
        .unwrap();
    let now = Utc::now();
    let _ = engine.tick(now).await;
    assert!(!invites_healthy(&pool).await);

    let stale_at = Utc::now() - Duration::seconds(901);
    set_marker(&pool, stale_at, Some(stale_at), true).await;
    let _ = engine.tick(Utc::now()).await;
    assert!(!invites_healthy(&pool).await);

    let future_at = Utc::now() + Duration::minutes(5);
    set_marker(&pool, future_at, Some(future_at), true).await;
    let _ = engine.tick(Utc::now()).await;
    assert!(!invites_healthy(&pool).await);

    let failed_at = Utc::now();
    set_marker(
        &pool,
        failed_at,
        Some(failed_at - Duration::seconds(1)),
        false,
    )
    .await;
    let _ = engine.tick(Utc::now()).await;
    assert!(!invites_healthy(&pool).await);

    let recovered_at = Utc::now();
    set_marker(&pool, recovered_at, Some(recovered_at), true).await;
    let _ = engine.tick(Utc::now()).await;
    assert!(invites_healthy(&pool).await);
    let invite_receipts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM partner_effort_source_receipts WHERE source='invites'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(invite_receipts, 0);

    drop(engine);
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
    admin.close().await;
}
