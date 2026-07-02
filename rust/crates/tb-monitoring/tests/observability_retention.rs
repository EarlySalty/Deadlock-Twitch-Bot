mod support;

use chrono::{Duration, Utc};
use tb_monitoring::cleanup_observability_events_before;

#[tokio::test]
async fn observability_retention_entfernt_alte_zeilen() {
    let Some(pool) = support::pool_in_schema("obs_retention_cleanup").await else {
        return;
    };
    sqlx::query(
        "CREATE TABLE twitch_observability_events (
            id BIGSERIAL,
            created_at TIMESTAMPTZ NOT NULL,
            flow_type TEXT,
            flow_id TEXT,
            entity_login TEXT,
            entity_id TEXT,
            step TEXT,
            decision TEXT,
            details_json JSONB
        )",
    )
    .execute(&pool)
    .await
    .expect("create observability table");

    let now = Utc::now();
    sqlx::query(
        "INSERT INTO twitch_observability_events \
         (created_at, flow_type, flow_id, step, decision, details_json) \
         VALUES ($1, 'raid', 'old', 's', 'd', '{}'::jsonb), \
                ($2, 'raid', 'new', 's', 'd', '{}'::jsonb)",
    )
    .bind(now - Duration::days(60))
    .bind(now - Duration::days(2))
    .execute(&pool)
    .await
    .expect("insert observability rows");

    let deleted = cleanup_observability_events_before(&pool, now - Duration::days(45))
        .await
        .expect("cleanup");
    assert_eq!(deleted, 1);

    let remaining: Vec<String> = sqlx::query_scalar(
        "SELECT flow_id FROM twitch_observability_events ORDER BY flow_id",
    )
    .fetch_all(&pool)
    .await
    .expect("remaining rows");
    assert_eq!(remaining, vec!["new".to_string()]);
}
