use std::str::FromStr;

use chrono::{TimeZone, Utc};
use serde_json::json;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_engagement::crew_review::{
    NewReviewEvent, ReviewEventKind, RickyChatInput, RICKY_TWITCH_USER_ID,
};
use tb_engagement::crew_review_store::CrewReviewStore;
use uuid::Uuid;

const MIGRATION: &str =
    include_str!("../../../migrations/20260717120000_twitch_crew_review_events.sql");
type TombstoneRow = (
    Option<String>,
    serde_json::Value,
    Option<String>,
    Option<String>,
    Option<f64>,
    String,
    Option<chrono::DateTime<Utc>>,
);

async fn test_pool(schema: &str) -> Option<PgPool> {
    let dsn = match std::env::var("TB_TEST_DATABASE_URL") {
        Ok(dsn) => dsn,
        Err(_) if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") => {
            panic!("TB_TEST_DATABASE_URL fehlt trotz TB_TEST_REQUIRE_DB=1")
        }
        Err(_) => {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return None;
        }
    };

    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("connect test postgres");
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&admin)
        .await
        .expect("drop test schema");
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .expect("create test schema");
    admin.close().await;

    let options = PgConnectOptions::from_str(&dsn)
        .expect("parse test postgres URL")
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .expect("connect isolated test schema");
    sqlx::raw_sql(MIGRATION)
        .execute(&pool)
        .await
        .expect("apply crew review migration");
    Some(pool)
}

fn input(
    channel: &str,
    source_message_id: &str,
    occurred_at: chrono::DateTime<Utc>,
) -> RickyChatInput {
    RickyChatInput {
        channel_login: channel.to_owned(),
        subject_twitch_user_id: RICKY_TWITCH_USER_ID.to_owned(),
        source_message_id: Some(source_message_id.to_owned()),
        occurred_at,
        content: "Ricky-Nachricht".to_owned(),
    }
}

fn event(
    session_id: Uuid,
    channel: &str,
    event_kind: ReviewEventKind,
    occurred_at: chrono::DateTime<Utc>,
    cycle_id: Uuid,
) -> NewReviewEvent {
    NewReviewEvent {
        session_id,
        channel_login: channel.to_owned(),
        subject_twitch_user_id: RICKY_TWITCH_USER_ID.to_owned(),
        event_kind,
        source_message_id: None,
        occurred_at,
        content: Some("Review-Inhalt".to_owned()),
        metadata: json!({"cycle_id": cycle_id.to_string(), "private": "remove-me"}),
        provider: Some("provider".to_owned()),
        model: Some("model".to_owned()),
        confidence: Some(0.75),
    }
}

#[test]
fn ricky_trigger_verwendet_die_stabile_twitch_id() {
    assert_eq!(RICKY_TWITCH_USER_ID, "147713656");
}

#[tokio::test]
async fn trigger_legt_session_und_nachricht_atomar_und_dedupliziert() {
    let Some(pool) = test_pool("crew_review_trigger").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap();
    let first = input("kanal_a", "msg-1", now);

    let session_id = store
        .record_trigger(&first)
        .await
        .expect("record first trigger")
        .expect("first trigger creates a cycle");
    assert!(store
        .record_trigger(&first)
        .await
        .expect("deduplicate trigger")
        .is_none());

    let second_cycle = store
        .record_trigger(&input(
            "kanal_a",
            "msg-2",
            now + chrono::Duration::minutes(9),
        ))
        .await
        .expect("record second trigger")
        .expect("new message creates cycle");
    assert_ne!(session_id, second_cycle, "record_trigger returns cycle IDs");

    let other_channel_cycle = store
        .record_trigger(&input("kanal_b", "msg-3", now))
        .await
        .expect("record other channel")
        .expect("other channel creates cycle");
    assert_ne!(session_id, other_channel_cycle);

    let mut wrong_subject = input("kanal_c", "msg-4", now);
    wrong_subject.subject_twitch_user_id = "someone-else".to_owned();
    assert!(store
        .record_trigger(&wrong_subject)
        .await
        .expect("ignore wrong subject")
        .is_none());

    let rows: Vec<(Uuid, String, String, serde_json::Value)> = sqlx::query_as(
        "SELECT review_session_id, channel_login, event_kind, metadata
           FROM twitch_crew_review_events
          ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].2, "session_started");
    assert_eq!(rows[1].2, "ricky_message");
    assert_eq!(rows[0].0, rows[1].0);
    assert_eq!(rows[0].3["cycle_id"], rows[1].3["cycle_id"]);
    assert_eq!(rows[1].3["cycle_id"], session_id.to_string());
    assert_eq!(rows[2].0, rows[0].0, "recent trigger reuses session");
    assert_ne!(rows[3].0, rows[0].0, "channels use separate sessions");
}

#[tokio::test]
async fn expires_at_sind_sechs_kalendermonate() {
    let Some(pool) = test_pool("crew_review_expiry").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let occurred_at = Utc.with_ymd_and_hms(2025, 8, 31, 9, 30, 0).unwrap();
    store
        .record_trigger(&input("expiry", "expiry-1", occurred_at))
        .await
        .unwrap();

    let expiries: Vec<(chrono::DateTime<Utc>, chrono::DateTime<Utc>)> =
        sqlx::query_as("SELECT occurred_at, expires_at FROM twitch_crew_review_events")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(expiries.iter().all(|(occurred, expires)| {
        *occurred == occurred_at && *expires == Utc.with_ymd_and_hms(2026, 2, 28, 9, 30, 0).unwrap()
    }));
}

#[tokio::test]
async fn tombstone_entfernt_inhalt_aber_behaelt_delete_retry() {
    let Some(pool) = test_pool("crew_review_tombstone").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = Uuid::new_v4();
    let event_id = store
        .append_event(event(
            session_id,
            "tombstone",
            ReviewEventKind::AiDraft,
            now - chrono::Duration::days(200),
            Uuid::new_v4(),
        ))
        .await
        .unwrap();
    store
        .mark_discord_sent(&[event_id], "discord-tombstone")
        .await
        .unwrap();

    let long_error = "provider_timeout_with_unbounded_raw_details_".repeat(4);
    store
        .tombstone_group("discord-tombstone", &long_error)
        .await
        .unwrap();

    let row: TombstoneRow = sqlx::query_as(
        "SELECT content, metadata, provider, model, confidence,
                last_delete_error, tombstoned_at
           FROM twitch_crew_review_events WHERE id = $1",
    )
    .bind(event_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, None);
    assert_eq!(row.2, None);
    assert_eq!(row.3, None);
    assert_eq!(row.4, None);
    assert!(row.5.chars().count() <= 64);
    assert_eq!(row.1.as_object().unwrap().len(), 2);
    assert_eq!(row.1["error_class"], row.5);
    assert!(row.1.get("tombstoned_at").is_some());
    assert!(row.1.get("private").is_none());
    assert!(row.1.get("cycle_id").is_none());
    assert!(row.6.is_some());
}

#[tokio::test]
async fn geloeschte_discord_gruppe_entfernt_erst_danach_db_zeilen() {
    let Some(pool) = test_pool("crew_review_delete_group").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let session_id = Uuid::new_v4();
    let cycle_id = Uuid::new_v4();
    let expired = Utc::now() - chrono::Duration::days(200);
    let fresh = Utc::now();
    let expired_id = store
        .append_event(event(
            session_id,
            "cleanup",
            ReviewEventKind::AiDecision,
            expired,
            cycle_id,
        ))
        .await
        .unwrap();
    let fresh_id = store
        .append_event(event(
            session_id,
            "cleanup",
            ReviewEventKind::AiDraft,
            fresh,
            cycle_id,
        ))
        .await
        .unwrap();
    store
        .mark_discord_sent(&[expired_id, fresh_id], "discord-mixed")
        .await
        .unwrap();

    assert!(store
        .expired_discord_groups(Utc::now(), 10)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        store.delete_expired_group("discord-mixed").await.unwrap(),
        0
    );
    let remaining: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM twitch_crew_review_events
          WHERE discord_message_id = 'discord-mixed' ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(remaining, vec![expired_id, fresh_id]);

    sqlx::query("UPDATE twitch_crew_review_events SET expires_at = NOW() - INTERVAL '1 second' WHERE id = $1")
        .bind(fresh_id)
        .execute(&pool)
        .await
        .unwrap();
    let groups = store.expired_discord_groups(Utc::now(), 10).await.unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].discord_message_id, "discord-mixed");
    assert_eq!(groups[0].event_ids, vec![expired_id, fresh_id]);
    assert_eq!(
        store.delete_expired_group("discord-mixed").await.unwrap(),
        2
    );
}

#[tokio::test]
async fn pending_queues_gruppieren_nur_unverarbeitete_und_ungepostete_events() {
    let Some(pool) = test_pool("crew_review_pending").await else {
        return;
    };
    let store = CrewReviewStore::new(pool);
    let now = Utc::now();
    let session_id = Uuid::new_v4();
    let pending_cycle = Uuid::new_v4();
    let completed_cycle = Uuid::new_v4();

    let pending_ricky = store
        .append_event(event(
            session_id,
            "pending",
            ReviewEventKind::RickyMessage,
            now,
            pending_cycle,
        ))
        .await
        .unwrap();
    let pending_transcript = store
        .append_event(event(
            session_id,
            "pending",
            ReviewEventKind::StreamerTranscript,
            now + chrono::Duration::seconds(1),
            pending_cycle,
        ))
        .await
        .unwrap();
    let completed_input = store
        .append_event(event(
            session_id,
            "pending",
            ReviewEventKind::RickyMessage,
            now + chrono::Duration::seconds(2),
            completed_cycle,
        ))
        .await
        .unwrap();
    let decision = store
        .append_event(event(
            session_id,
            "pending",
            ReviewEventKind::AiDecision,
            now + chrono::Duration::seconds(3),
            completed_cycle,
        ))
        .await
        .unwrap();

    let model_inputs = store.pending_model_inputs(session_id).await.unwrap();
    assert_eq!(
        model_inputs
            .iter()
            .map(|event| event.id)
            .collect::<Vec<_>>(),
        vec![pending_ricky, pending_transcript]
    );

    store
        .mark_discord_sent(&[pending_ricky, completed_input], "discord-posted")
        .await
        .unwrap();
    let cycles = store.pending_discord_cycles(10).await.unwrap();
    assert_eq!(cycles.len(), 1);
    assert_eq!(cycles[0].cycle_id, completed_cycle);
    assert_eq!(cycles[0].session_id, session_id);
    assert_eq!(cycles[0].channel_login, "pending");
    assert_eq!(cycles[0].events.len(), 1);
    assert_eq!(cycles[0].events[0].id, decision);
}

#[tokio::test]
async fn lange_inhalte_werden_an_wortgrenzen_geteilt() {
    let Some(pool) = test_pool("crew_review_chunks").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let mut long = "wort ".repeat(260);
    long.push_str("ende");
    let cycle_id = Uuid::new_v4();
    let mut new_event = event(
        Uuid::new_v4(),
        "chunks",
        ReviewEventKind::RickyMessage,
        Utc::now(),
        cycle_id,
    );
    new_event.source_message_id = Some("chunk-source".to_owned());
    new_event.content = Some(long.clone());
    store.append_event(new_event).await.unwrap();

    let chunks: Vec<(Option<String>, String, serde_json::Value)> = sqlx::query_as(
        "SELECT source_message_id, content, metadata
           FROM twitch_crew_review_events ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].0.as_deref(), Some("chunk-source"));
    assert_eq!(chunks[1].0, None);
    assert!(chunks.iter().all(|(_, content, _)| content.len() <= 1200));
    assert_eq!(chunks[0].2["chunk_index"], 0);
    assert_eq!(chunks[1].2["chunk_index"], 1);
    assert_eq!(chunks[0].2["chunk_count"], 2);
    assert_eq!(
        chunks
            .iter()
            .map(|(_, content, _)| content.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        long.trim()
    );
}
