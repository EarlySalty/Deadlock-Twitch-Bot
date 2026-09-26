//! Real isolated PostgreSQL; never uses production or skips the storage contract.
#[path = "../../../test-support/postgres.rs"]
mod test_postgres;

use chrono::{Duration, Utc};
use sqlx::Row;
use tb_analytics::category;
use tb_transport_twitch::streams::HelixStream;
use test_postgres::TestPostgres;

#[tokio::test]
async fn category_storage_deletions_rollups_preservation_and_report() {
    let db = TestPostgres::start().await;
    let pool = &db.pool;
    for migration in [
        include_str!("../../../migrations/20260918123000_category_collector.sql"),
        include_str!("../../../migrations/20260918170000_category_permanent_archive.sql"),
    ] {
        sqlx::raw_sql(migration).execute(pool).await.unwrap();
    }
    let now = Utc::now();
    let streams = vec![HelixStream {
        id: "s1".into(),
        user_id: "r1".into(),
        user_login: "sample".into(),
        user_name: "Sample".into(),
        game_id: "deadlock".into(),
        language: "de".into(),
        viewer_count: 100,
        title: "Synthetic test".into(),
        started_at: (now - Duration::hours(1)).to_rfc3339(),
        ..Default::default()
    }];
    category::store_snapshot(pool, now - Duration::seconds(61), &streams, 60)
        .await
        .unwrap();
    category::store_snapshot(pool, now - Duration::seconds(1), &streams, 60)
        .await
        .unwrap();
    let line = |id: &str| {
        format!("@room-id=r1;user-id=u1;id={id};tmi-sent-ts={} :sampleuser!u@u PRIVMSG #sample :Dies ist eine vollständige deutsche Nachricht für einen sicheren Datenbanktest.",now.timestamp_millis())
    };
    let first = category::raw_message(&line("m1"), now, "r1", "de").unwrap();
    let second = category::raw_message(&line("m2"), now, "r1", "de").unwrap();
    let shared_line = line("copy").replace("@room-id=", "@source-room-id=other;room-id=");
    let shared = category::raw_message(&shared_line, now, "r1", "de").unwrap();
    assert_eq!(
        category::store_messages(pool, &[first.clone(), first.clone(), second, shared])
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        category::store_messages(pool, &[first.clone()])
            .await
            .unwrap(),
        0
    );
    assert_eq!(category::flush_rollups(pool, 100).await.unwrap(), 1);
    let row = sqlx::query("SELECT messages,distinct_chatter FROM category_chat_rollup")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(row.get::<i64, _>("messages"), 2);
    assert_eq!(row.get::<i64, _>("distinct_chatter"), 1);
    assert_eq!(category::flush_rollups(pool, 100).await.unwrap(), 0);
    assert_eq!(
        category::delete_chat(
            pool,
            "@room-id=r1;target-msg-id=m1 :tmi.twitch.tv CLEARMSG #sample :deleted",
            "r1",
            Utc::now(),
        )
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        category::store_messages(pool, &[first]).await.unwrap(),
        0,
        "a late duplicate cannot resurrect removed text"
    );
    category::flush_rollups(pool, 100).await.unwrap();
    let report = category::report(pool, 7).await.unwrap();
    assert_eq!(report["languages"][0]["language"], "de");
    assert_eq!(report["languages"][0]["streams"], 1);
    assert_eq!(report["languages"][0]["messages"], 1);
    assert_eq!(report["trend"][0]["viewers"], 100.0);
    assert!((report["languages"][0]["airtime_hours"].as_f64().unwrap() - 1.0 / 60.0).abs() < 1e-8);
    assert_eq!(report["method"]["region"], "language_not_geography");
    let output = report.to_string();
    assert!(!output.contains("message_text"));
    assert!(!output.contains("chatter_user_id"));
    category::delete_chat(
        pool,
        "@room-id=r1;target-user-id=u1 :tmi.twitch.tv CLEARCHAT #sample :sampleuser",
        "r1",
        Utc::now(),
    )
    .await
    .unwrap();
    category::flush_rollups(pool, 100).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM category_chat_messages")
            .fetch_one(pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT messages FROM category_chat_rollup")
            .fetch_one(pool)
            .await
            .unwrap(),
        0
    );

    let day = (now - Duration::days(100)).date_naive();
    let name = format!("category_chat_messages_p{}", day.format("%Y%m%d"));
    sqlx::raw_sql(&format!("CREATE TABLE {name} PARTITION OF category_chat_messages FOR VALUES FROM ('{day} 00:00:00+00') TO ('{} 00:00:00+00'); INSERT INTO category_chat_dirty VALUES('{day} 00:00:00+00','old','de');",day.succ_opt().unwrap())).execute(pool).await.unwrap();
    category::flush_rollups(pool, 100).await.unwrap();
    for days in [1, 90, 91, 1000] {
        let dropped: i32 =
            sqlx::query_scalar("SELECT dropped FROM category_prune_partitions($1,21474836480)")
                .bind(days)
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(dropped, 0, "age is never permission to destroy an archive");
    }
    assert_eq!(category::trim_expired_rows(pool, 90).await.unwrap(), 0);
    let exists: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
        .bind(name)
        .fetch_one(pool)
        .await
        .unwrap();
    assert!(exists, "old partitions must remain present");
}

#[tokio::test]
async fn explicit_source_removal_also_covers_late_shared_chat_copies() {
    let db = TestPostgres::start().await;
    for migration in [
        include_str!("../../../migrations/20260918123000_category_collector.sql"),
        include_str!("../../../migrations/20260918170000_category_permanent_archive.sql"),
    ] {
        sqlx::raw_sql(migration).execute(&db.pool).await.unwrap();
    }
    let now = Utc::now();
    let copied = format!("@room-id=20;user-id=30;id=copy;source-room-id=10;source-id=original;tmi-sent-ts={} :viewer!v@v PRIVMSG #sample :Eine kopierte Nachricht für den kontrollierten Datenbanktest.",now.timestamp_millis());
    let row = category::raw_message(&copied, now, "20", "de").unwrap();
    assert_eq!(
        category::store_messages(&db.pool, &[row.clone()])
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        category::delete_chat(
            &db.pool,
            "@room-id=10;target-msg-id=original :tmi.twitch.tv CLEARMSG #source :deleted",
            "10",
            Utc::now(),
        )
        .await
        .unwrap(),
        1
    );
    assert_eq!(category::store_messages(&db.pool, &[row]).await.unwrap(), 0);
    assert_eq!(
        category::delete_chat(
            &db.pool,
            "@room-id=10 :tmi.twitch.tv CLEARCHAT #source",
            "10",
            Utc::now(),
        )
        .await
        .unwrap(),
        0
    );
}
