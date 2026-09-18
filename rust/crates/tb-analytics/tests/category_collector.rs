//! Integration contract against an explicitly provisioned, disposable database.
//! Never skips silently and never defaults to the production database.
use chrono::{Duration, Utc};
use sqlx::{postgres::PgPoolOptions, Row};
use tb_analytics::category;
use tb_transport_twitch::streams::HelixStream;

#[tokio::test]
#[ignore = "requires disposable database configured in /tmp/tb-category-test-dsn"]
async fn category_storage_deletions_rollups_retention_and_report() {
    let dsn =
        std::fs::read_to_string("/tmp/tb-category-test-dsn").expect("explicit disposable test DSN");
    assert!(
        dsn.contains("categorytest_"),
        "production database is forbidden"
    );
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect(dsn.trim())
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/20260918123000_category_collector.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
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
    category::store_snapshot(&pool, now - Duration::seconds(61), &streams, 60)
        .await
        .unwrap();
    category::store_snapshot(&pool, now - Duration::seconds(1), &streams, 60)
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
        category::store_messages(&pool, &[first.clone(), first.clone(), second, shared])
            .await
            .unwrap(),
        3
    );
    assert_eq!(category::store_messages(&pool, &[first]).await.unwrap(), 0);
    assert_eq!(category::flush_rollups(&pool, 100).await.unwrap(), 1);
    let row = sqlx::query("SELECT messages,distinct_chatter FROM category_chat_rollup")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<i64, _>("messages"), 2);
    assert_eq!(row.get::<i64, _>("distinct_chatter"), 1);
    assert_eq!(category::flush_rollups(&pool, 100).await.unwrap(), 0);
    category::delete_chat(
        &pool,
        "@room-id=r1;target-msg-id=m1 :tmi.twitch.tv CLEARMSG #sample :deleted",
        "r1",
    )
    .await
    .unwrap();
    category::flush_rollups(&pool, 100).await.unwrap();
    let messages: i64 = sqlx::query_scalar("SELECT messages FROM category_chat_rollup")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(messages, 1);
    let report = category::report(&pool, 7).await.unwrap();
    assert_eq!(report["languages"][0]["language"], "de");
    assert_eq!(report["languages"][0]["streams"], 1);
    assert_eq!(report["languages"][0]["messages"], 1);
    assert_eq!(report["trend"][0]["viewers"], 100.0);
    assert!((report["languages"][0]["airtime_hours"].as_f64().unwrap() - 1.0 / 60.0).abs() < 1e-8);
    assert_eq!(report["method"]["region"], "language_not_geography");
    category::delete_chat(
        &pool,
        "@room-id=r1;target-user-id=u1 :tmi.twitch.tv CLEARCHAT #sample :sampleuser",
        "r1",
    )
    .await
    .unwrap();
    category::flush_rollups(&pool, 100).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM category_chat_messages")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT messages FROM category_chat_rollup")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    // A fully expired partition is actually dropped, but only after its pending
    // aggregation has been finalized. Rollup rows themselves must survive.
    let day = (now - Duration::days(100)).date_naive();
    let name = format!("category_chat_messages_p{}", day.format("%Y%m%d"));
    sqlx::raw_sql(&format!("CREATE TABLE {name} PARTITION OF category_chat_messages FOR VALUES FROM ('{day} 00:00:00+00') TO ('{} 00:00:00+00');
        INSERT INTO category_chat_dirty VALUES('{day} 00:00:00+00','old','de');",day.succ_opt().unwrap())).execute(&pool).await.unwrap();
    let dropped: i32 =
        sqlx::query_scalar("SELECT dropped FROM category_prune_partitions(90,21474836480)")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(dropped, 0, "unprocessed buckets cannot be pruned");
    category::flush_rollups(&pool, 100).await.unwrap();
    let dropped: i32 =
        sqlx::query_scalar("SELECT dropped FROM category_prune_partitions(90,21474836480)")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(dropped, 1);
    let retained: i64 =
        sqlx::query_scalar("SELECT count(*) FROM category_chat_rollup WHERE room_user_id='old'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(retained, 1);
    assert!(
        sqlx::query("SELECT * FROM category_prune_partitions(91,21474836480)")
            .fetch_all(&pool)
            .await
            .is_err()
    );
    pool.close().await;
}
