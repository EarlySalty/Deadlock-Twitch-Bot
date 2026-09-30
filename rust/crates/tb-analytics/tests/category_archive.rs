//! Real isolated PostgreSQL. No production DSN, environment or skipped tests.
#[path = "../../../test-support/postgres.rs"]
mod test_postgres;

use chrono::{Duration, Utc};
use tb_analytics::category;
use tb_transport_twitch::streams::HelixStream;
use test_postgres::TestPostgres;

async fn fixture() -> TestPostgres {
    let db = TestPostgres::start().await;
    sqlx::raw_sql(include_str!("../../../migrations/20260918123000_category_collector.sql"))
        .execute(&db.pool).await.unwrap();
    sqlx::raw_sql(include_str!("../../../migrations/20260918170000_category_permanent_archive.sql"))
        .execute(&db.pool).await.unwrap();
    let old = Utc::now() - Duration::days(1000);
    let day = old.date_naive();
    sqlx::raw_sql(&format!(
        "CREATE TABLE category_chat_messages_p{} PARTITION OF category_chat_messages FOR VALUES FROM ('{day} 00:00:00+00') TO ('{} 00:00:00+00')",
        day.format("%Y%m%d"), day.succ_opt().unwrap()
    )).execute(&db.pool).await.unwrap();
    for (id, at) in [("archive", old), ("recent", Utc::now())] {
        let line = format!("@room-id=100;user-id=200;id={id};tmi-sent-ts={} :viewer!v@v PRIVMSG #sample :Diese Nachricht bleibt für die spätere Auswertung im Archiv erhalten.", at.timestamp_millis());
        let row = category::raw_message(&line, at, "100", "de").unwrap();
        assert_eq!(category::store_messages(&db.pool, &[row]).await.unwrap(), 1);
    }
    category::flush_rollups(&db.pool, 100).await.unwrap();
    db
}

async fn count(db: &TestPostgres) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM category_chat_messages")
        .fetch_one(&db.pool).await.unwrap()
}

#[tokio::test]
async fn old_row_pruning_is_inert_even_for_legacy_callers() {
    let db = fixture().await;
    assert_eq!(category::trim_expired_rows(&db.pool, 90).await.unwrap(), 0);
    assert_eq!(count(&db).await, 2);
}

#[tokio::test]
async fn old_partition_pruning_and_storage_pressure_never_delete_archives() {
    let db = fixture().await;
    for (days, budget) in [(90, 21_474_836_480_i64), (1, 1), (1001, 1)] {
        let result: (i32, bool, i64) = sqlx::query_as(
            "SELECT dropped, pressure, raw_bytes FROM category_prune_partitions($1,$2)"
        ).bind(days).bind(budget).fetch_one(&db.pool).await.unwrap();
        assert_eq!(result.0, 0, "the compatibility function never drops a partition");
        assert!(result.2 > 0);
        if budget == 1 { assert!(result.1, "storage pressure must still be visible"); }
        assert_eq!(count(&db).await, 2);
    }
}

#[tokio::test]
async fn clearing_a_room_is_not_permission_to_destroy_its_archive() {
    let db = fixture().await;
    category::delete_chat(&db.pool,
        "@room-id=100 :tmi.twitch.tv CLEARCHAT #sample", "100").await.unwrap();
    assert_eq!(count(&db).await, 2);
    category::flush_rollups(&db.pool, 100).await.unwrap();
    let total: i64 = sqlx::query_scalar("SELECT sum(messages)::bigint FROM category_chat_rollup")
        .fetch_one(&db.pool).await.unwrap();
    assert_eq!(total, 2);
}

#[tokio::test]
async fn a_targeted_chat_clear_is_limited_to_the_observed_stream() {
    let db = fixture().await;
    let now = Utc::now();
    category::store_snapshot(&db.pool, now, &[HelixStream {
        id: "stream".into(), user_id: "100".into(), user_login: "sample".into(),
        user_name: "Sample".into(), language: "de".into(), game_id: "deadlock".into(),
        started_at: (now-Duration::hours(1)).to_rfc3339(), ..Default::default()
    }], 60).await.unwrap();
    category::delete_chat(&db.pool,
        "@room-id=100;target-user-id=200 :tmi.twitch.tv CLEARCHAT #sample :viewer", "100").await.unwrap();
    assert_eq!(count(&db).await, 1, "a timeout must not erase previous streams");
    let remaining: String = sqlx::query_scalar("SELECT message_id FROM category_chat_messages")
        .fetch_one(&db.pool).await.unwrap();
    assert_eq!(remaining, "archive");
    category::delete_chat(&db.pool,
        "@room-id=100;target-msg-id=archive :tmi.twitch.tv CLEARMSG #sample :deleted", "100").await.unwrap();
    assert_eq!(count(&db).await, 0, "an explicit message deletion still works");
}
