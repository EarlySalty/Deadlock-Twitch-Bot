//! Hermetischer DB-Test des deferred Observability-Writers gegen
//! `twitch_observability_events`. Erstellt ein eigenes Schema und prüft, dass
//! sowohl Raid- als auch Analytics-Flows persistierte Zeilen erzeugen.

use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{PgPool, Row};
use tb_observability::{
    AnalyticsDecision, AnalyticsObservabilityService, ObservabilityWriter,
    RaidObservabilityService,
};

macro_rules! pool_or_skip {
    ($schema:expr) => {{
        let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        pool_in_schema(&dsn, $schema).await
    }};
}

async fn pool_in_schema(dsn: &str, schema: &str) -> PgPool {
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(dsn)
        .await
        .unwrap();
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    let opts = PgConnectOptions::from_str(dsn)
        .unwrap()
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE twitch_observability_events (
            id BIGSERIAL PRIMARY KEY,
            flow_type TEXT NOT NULL,
            flow_id TEXT NOT NULL,
            entity_login TEXT,
            entity_id TEXT,
            step TEXT NOT NULL,
            decision TEXT NOT NULL,
            details_json TEXT NOT NULL DEFAULT '{}',
            created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

async fn wait_for_rows(pool: &PgPool, expected: i64) -> i64 {
    for _ in 0..50 {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_observability_events")
                .fetch_one(pool)
                .await
                .unwrap();
        if count >= expected {
            return count;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    sqlx::query_scalar("SELECT COUNT(*) FROM twitch_observability_events")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn raid_flow_writes_per_step_rows() {
    let pool = pool_or_skip!("obs_raid_writer");
    let writer = ObservabilityWriter::spawn(pool.clone(), 100, 10);
    let svc = RaidObservabilityService::new(Some(Arc::new(writer.clone())));

    let flow_id = svc.next_flow_id("raid");
    for step in ["started", "selected", "executed"] {
        let mut details = BTreeMap::new();
        details.insert("step_marker".to_string(), json!(step));
        svc.emit_event(
            "raid",
            &flow_id,
            step,
            "success",
            Some("source_login"),
            Some("111"),
            Some("target_login"),
            Some("222"),
            details,
        );
    }
    // failed-Events dürfen NICHT persistiert werden.
    svc.emit_event(
        "raid",
        &flow_id,
        "aborted",
        "failed",
        None,
        None,
        Some("target_login"),
        Some("222"),
        BTreeMap::new(),
    );

    drop(writer);
    drop(svc);

    let count = wait_for_rows(&pool, 3).await;
    assert_eq!(count, 3, "exactly 3 success rows, failed event dropped");

    let row = sqlx::query(
        "SELECT flow_type, flow_id, entity_login, entity_id, step, decision \
         FROM twitch_observability_events ORDER BY id LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("flow_type"), "raid");
    assert_eq!(row.get::<String, _>("flow_id"), flow_id);
    assert_eq!(row.get::<String, _>("entity_login"), "target_login");
    assert_eq!(row.get::<String, _>("entity_id"), "222");
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn analytics_terminal_decision_row_written() {
    let pool = pool_or_skip!("obs_analytics_writer");
    let writer = ObservabilityWriter::spawn(pool.clone(), 100, 10);
    let svc = AnalyticsObservabilityService::new(Some(Arc::new(writer.clone())), true, true, true);

    let flow_id = svc.next_flow_id("chatters");
    let mut decision = AnalyticsDecision {
        flow_id,
        flow: "chatters".into(),
        login: "streamername".into(),
        session_id: Some(77),
        decision: "success".into(),
        reason: "bot_path_success".into(),
        request_attempted: Some(true),
        request_result: "success".into(),
        http_status: Some(200),
        scope_state: BTreeMap::new(),
        runtime_state: BTreeMap::new(),
        extra: BTreeMap::new(),
    };
    decision
        .extra
        .insert("chatter_count".to_string(), json!(5));
    svc.log_decision(decision);

    drop(writer);
    drop(svc);

    let count = wait_for_rows(&pool, 1).await;
    assert_eq!(count, 1);

    let row = sqlx::query(
        "SELECT flow_type, step, decision, entity_login, entity_id, details_json \
         FROM twitch_observability_events ORDER BY id LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("flow_type"), "analytics");
    assert_eq!(row.get::<String, _>("step"), "terminal_decision");
    assert_eq!(row.get::<String, _>("decision"), "success");
    assert_eq!(row.get::<String, _>("entity_login"), "streamername");
    assert_eq!(row.get::<String, _>("entity_id"), "77");
    let details: String = row.get("details_json");
    assert!(details.contains("\"chatter_count\":5"));
    pool.close().await;
}
