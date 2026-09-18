//! These tests start private Unix-socket PostgreSQL instances. They never
//! read a production DSN, never silently skip, and contain synthetic data only.
use crate::{
    config,
    store::{self, ChatZeile},
    test_database::Database,
};
use chrono::{DateTime, Utc};
use serde_json::json;
use tb_transport_twitch::{streams::HelixStream, HelixClient, HelixConfig};

async fn database() -> Database {
    let db = Database::new().await;
    sqlx::raw_sql(include_str!(
        "../../../migrations/20260918123000_category_collector.sql"
    ))
    .execute(&db.pool)
    .await
    .unwrap();
    db
}
fn hour(offset: i64) -> DateTime<Utc> {
    DateTime::from_timestamp((Utc::now().timestamp() / 3600 + offset) * 3600, 0).unwrap()
}
fn chat(id: &str, at: DateTime<Utc>) -> ChatZeile {
    ChatZeile {
        room_user_id: "99".into(),
        message_id: id.into(),
        source_message_id: None,
        source_room_id: None,
        sent_at: at,
        chatter_user_id: "42".into(),
        chatter_login: "fixture".into(),
        message_text: "example text".into(),
        text_len: 12,
        detected_lang: "en".into(),
        lang_confidence: Some(0.9),
        lang_method: "fixture".into(),
        emote_count: 0,
        irc_tags: json!({}),
    }
}
fn stream(id: &str) -> HelixStream {
    HelixStream {
        id: id.into(),
        user_id: "99".into(),
        user_login: "channel".into(),
        user_name: "Channel".into(),
        game_id: "game".into(),
        game_name: "Deadlock".into(),
        title: "Fixture".into(),
        language: "de".into(),
        viewer_count: 100,
        is_mature: false,
        tags: Some(vec!["Deutsch".into()]),
        started_at: hour(-3).to_rfc3339(),
        thumbnail_url: "https://example.invalid/thumbnail".into(),
    }
}

#[tokio::test]
async fn snapshots_commit_atomically_and_are_idempotent() {
    let db = database().await;
    let at = hour(-1);
    let id = store::start_poll(&db.pool, at).await.unwrap();
    store::complete_poll(&db.pool, id, at, &[stream("s1")], 1)
        .await
        .unwrap();
    store::complete_poll(&db.pool, id, at, &[stream("s1")], 1)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM category_stream_snapshots")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let status: String = sqlx::query_scalar("SELECT status FROM category_polls WHERE poll_id=$1")
        .bind(id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(status, "complete");
    let thumbnail: String =
        sqlx::query_scalar("SELECT thumbnail_url FROM category_stream_snapshots")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert!(thumbnail.contains("example.invalid"));
    let broken = store::start_poll(&db.pool, at).await.unwrap();
    let mut invalid = stream("bad");
    invalid.started_at = "not a timestamp".into();
    assert!(store::complete_poll(&db.pool, broken, at, &[invalid], 1)
        .await
        .is_err());
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM category_stream_snapshots WHERE poll_id=$1")
            .bind(broken)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    db.close().await;
}

#[tokio::test]
async fn duplicates_and_shared_chat_do_not_inflate_original_activity() {
    let db = database().await;
    let mut original = chat("original", hour(-1));
    original.source_message_id = Some("original".into());
    original.source_room_id = Some("99".into());
    let mut forwarded = chat("forwarded", hour(-1));
    forwarded.source_room_id = Some("100".into());
    let (inserted, duplicates, expired) =
        store::insert_chat(&db.pool, &[original.clone(), original, forwarded])
            .await
            .unwrap();
    assert_eq!((inserted, duplicates, expired), (2, 1, 0));
    assert!(store::rollup_one(&db.pool).await.unwrap());
    assert!(!store::rollup_one(&db.pool).await.unwrap());
    let counts: (i64, i64) =
        sqlx::query_as("SELECT messages,distinct_chatters FROM category_chat_rollup")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(counts, (1, 1));
    let raw: i64 = sqlx::query_scalar("SELECT count(*) FROM category_chat_messages")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        raw, 2,
        "forwarded raw message is retained, not silently discarded"
    );
    db.close().await;
}

#[tokio::test]
async fn late_messages_requeue_a_closed_hour_without_adding_distinct_counts() {
    let db = database().await;
    store::insert_chat(&db.pool, &[chat("one", hour(-2))])
        .await
        .unwrap();
    store::rollup_one(&db.pool).await.unwrap();
    store::insert_chat(
        &db.pool,
        &[chat("two", hour(-2) + chrono::Duration::minutes(1))],
    )
    .await
    .unwrap();
    store::rollup_one(&db.pool).await.unwrap();
    let counts: (i64, i64) =
        sqlx::query_as("SELECT messages,distinct_chatters FROM category_chat_rollup")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(counts, (2, 1));
    db.close().await;
}

#[tokio::test]
async fn paused_collection_still_expires_raw_data_only_after_rollup() {
    let db = database().await;
    let old = hour(-24 * 91);
    sqlx::query("INSERT INTO category_chat_messages(room_user_id,message_id,sent_at,chatter_user_id,chatter_login,message_text,text_len,detected_lang)
        VALUES('99','expired',$1,'42','fixture','old text',8,'en')")
        .bind(old).execute(&db.pool).await.unwrap();
    sqlx::query("INSERT INTO category_chat_dirty_hours VALUES($1)")
        .bind(old)
        .execute(&db.pool)
        .await
        .unwrap();
    store::insert_chat(&db.pool, &[chat("recent", hour(-1))])
        .await
        .unwrap();
    sqlx::query("UPDATE category_collector_config SET enabled=false")
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        store::retention_batch(&db.pool).await.unwrap(),
        0,
        "unaggregated history must survive"
    );
    store::rollup_one(&db.pool).await.unwrap();
    assert_eq!(store::retention_batch(&db.pool).await.unwrap(), 1);
    let raw: i64 = sqlx::query_scalar("SELECT count(*) FROM category_chat_messages")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    let durable: i64 =
        sqlx::query_scalar("SELECT messages FROM category_chat_rollup WHERE hour_bucket=$1")
            .bind(old)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(raw, 1);
    assert_eq!(durable, 1);
    assert_eq!(
        store::insert_chat(&db.pool, &[chat("too-old", old)])
            .await
            .unwrap(),
        (0, 0, 1)
    );
    db.close().await;
}

#[tokio::test]
async fn deleting_chat_text_does_not_erase_future_messages_or_counts() {
    let db = database().await;
    let at = hour(-1);
    store::insert_chat(
        &db.pool,
        &[
            chat("before", at),
            chat("after", at + chrono::Duration::minutes(2)),
        ],
    )
    .await
    .unwrap();
    assert_eq!(
        store::redact(
            &db.pool,
            "99",
            None,
            Some("42"),
            at + chrono::Duration::minutes(1)
        )
        .await
        .unwrap(),
        1
    );
    let text: String = sqlx::query_scalar(
        "SELECT message_text FROM category_chat_messages WHERE message_id='after'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(text, "example text");
    store::rollup_one(&db.pool).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT messages FROM category_chat_rollup")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
    db.close().await;
}

#[tokio::test]
async fn retention_limits_and_missing_configuration_fail_closed() {
    let db = database().await;
    for value in [0, 91, 365] {
        assert!(
            sqlx::query("UPDATE category_collector_config SET retention_days=$1")
                .bind(value)
                .execute(&db.pool)
                .await
                .is_err()
        );
    }
    assert_eq!(config::lade(&db.pool).await.unwrap().retention_days, 90);
    sqlx::query("DELETE FROM category_collector_config")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(config::lade(&db.pool).await.is_err());
    db.close().await;
}

#[tokio::test]
async fn singleton_lock_rejects_a_second_collector_before_any_network_access() {
    let db = database().await;
    let first = crate::collector::SammlerDienst::new(
        db.pool.clone(),
        HelixClient::new(HelixConfig::new("fixture", "fixture")).unwrap(),
    )
    .await
    .unwrap();
    let second = crate::collector::SammlerDienst::new(
        db.pool.clone(),
        HelixClient::new(HelixConfig::new("fixture", "fixture")).unwrap(),
    )
    .await;
    assert!(second.is_err());
    drop(first);
    db.close().await;
}

#[tokio::test]
async fn status_updates_are_idempotent_and_resume_from_saved_baseline() {
    let db = database().await;
    let baseline = store::counter_baseline(&db.pool).await.unwrap();
    let mut stats = tb_monitoring::anon_chat::AnonChatStatsSnapshot {
        privmsgs_dropped: 7,
        ..Default::default()
    };
    store::status(&db.pool, &stats, &baseline, 3, 2)
        .await
        .unwrap();
    store::status(&db.pool, &stats, &baseline, 3, 2)
        .await
        .unwrap();
    let totals: (i64, i64) = sqlx::query_as(
        "SELECT chat_privmsgs_dropped,chat_queue_dropped FROM category_collector_status",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(totals, (7, 3));
    let restarted = store::counter_baseline(&db.pool).await.unwrap();
    stats.privmsgs_dropped = 1;
    store::status(&db.pool, &stats, &restarted, 1, 0)
        .await
        .unwrap();
    let totals: (i64, i64) = sqlx::query_as(
        "SELECT chat_privmsgs_dropped,chat_queue_dropped FROM category_collector_status",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(totals, (8, 4));
    db.close().await;
}

#[tokio::test]
async fn dashboard_cannot_read_raw_text_or_chatter_ids_and_collector_cannot_change_retention() {
    let db = database().await;
    sqlx::raw_sql("CREATE ROLE twitchbot;CREATE ROLE twitchdash;CREATE ROLE twitchlegacy;")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../ops/systemd/category-collector-runtime.sql"
    ))
    .execute(&db.pool)
    .await
    .unwrap();
    for statement in [
        "SELECT message_text FROM category_chat_messages",
        "SELECT chatter_user_id FROM category_chat_messages",
        "SELECT irc_tags FROM category_chat_messages",
    ] {
        let mut tx = db.pool.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE twitchdash")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(sqlx::query(statement).execute(&mut *tx).await.is_err());
        tx.rollback().await.unwrap();
    }
    let mut tx = db.pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE twitchdash")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT detected_lang,count(*) FROM category_chat_messages WHERE source_room_id IS NULL GROUP BY detected_lang")
        .execute(&mut *tx).await.unwrap();
    tx.rollback().await.unwrap();
    let mut tx = db.pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE twitchcollector")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE category_collector_config SET retention_days=1")
            .execute(&mut *tx)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    db.close().await;
}
