use std::{str::FromStr, time::Instant};

use chrono::{TimeZone, Utc};
use serde_json::json;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_engagement::crew_review::{
    NewReviewEvent, ReviewEventKind, RickyChatInput, RICKY_TWITCH_USER_ID,
};
use tb_engagement::crew_review_store::{CrewReviewStore, DiscordCard, StoreError};
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

async fn seed_session(pool: &PgPool, channel: &str, occurred_at: chrono::DateTime<Utc>) -> Uuid {
    let session_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO twitch_crew_review_events (
            review_session_id, channel_login, subject_twitch_user_id,
            event_kind, occurred_at, metadata, expires_at
         ) VALUES ($1, $2, $3, 'session_started', $4, $5, $4 + INTERVAL '6 months')",
    )
    .bind(session_id)
    .bind(channel)
    .bind(RICKY_TWITCH_USER_ID)
    .bind(occurred_at)
    .bind(json!({"cycle_id": Uuid::new_v4().to_string()}))
    .execute(pool)
    .await
    .unwrap();
    session_id
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

async fn wait_for_advisory_query(pool: &PgPool, query_fragment: &str) {
    let reached = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS (
                    SELECT 1
                      FROM pg_stat_activity
                     WHERE datname = current_database()
                       AND wait_event = 'advisory'
                       AND query LIKE '%' || $1 || '%'
                )",
            )
            .bind(query_fragment)
            .fetch_one(pool)
            .await
            .expect("inspect advisory waiters");
            if waiting {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    if reached.is_err() {
        let waiting: Vec<String> = sqlx::query_scalar(
            "SELECT query FROM pg_stat_activity
              WHERE datname = current_database() AND wait_event = 'advisory'",
        )
        .fetch_all(pool)
        .await
        .unwrap();
        panic!("advisory waiter did not reach barrier {query_fragment}: {waiting:?}");
    }
}

async fn wait_for_lock_query(pool: &PgPool, query_fragment: &str) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS (
                    SELECT 1
                      FROM pg_stat_activity
                     WHERE datname = current_database()
                       AND wait_event_type = 'Lock'
                       AND query LIKE '%' || $1 || '%'
                )",
            )
            .bind(query_fragment)
            .fetch_one(pool)
            .await
            .expect("inspect lock waiters");
            if waiting {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("query did not reach lock barrier");
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
async fn session_aktivitaet_folgt_dem_metadata_vertrag_und_wird_wiederverwendet() {
    let Some(pool) = test_pool("crew_review_session_activity").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let started_at = Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap();
    let first_cycle = store
        .record_trigger(&input("activity", "activity-1", started_at))
        .await
        .unwrap()
        .unwrap();
    let session_id: Uuid = sqlx::query_scalar(
        "SELECT review_session_id
           FROM twitch_crew_review_events
          WHERE metadata->>'cycle_id' = $1
            AND event_kind = 'ricky_message'",
    )
    .bind(first_cycle.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();

    let transcript_at = started_at + chrono::Duration::minutes(9);
    let mut transcript = event(
        session_id,
        "activity",
        ReviewEventKind::StreamerTranscript,
        transcript_at,
        Uuid::new_v4(),
    );
    transcript.metadata["subject_mentioned"] = json!(true);
    store.append_event(transcript).await.unwrap();

    let sessions = store
        .active_sessions(started_at + chrono::Duration::minutes(18))
        .await
        .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].last_activity_at, transcript_at);

    let decision_at = started_at + chrono::Duration::minutes(18);
    let mut decision = event(
        session_id,
        "activity",
        ReviewEventKind::AiDecision,
        decision_at,
        Uuid::new_v4(),
    );
    decision.metadata["topic_active"] = json!(true);
    store.append_event(decision).await.unwrap();

    for (event_kind, metadata, minute) in [
        (
            ReviewEventKind::StreamerTranscript,
            json!({"cycle_id": Uuid::new_v4().to_string(), "subject_mentioned": false}),
            19,
        ),
        (
            ReviewEventKind::StreamerTranscript,
            json!({"cycle_id": Uuid::new_v4().to_string(), "subject_mentioned": "true"}),
            20,
        ),
        (
            ReviewEventKind::StreamerTranscript,
            json!({"cycle_id": Uuid::new_v4().to_string()}),
            21,
        ),
        (
            ReviewEventKind::AiDecision,
            json!({"cycle_id": Uuid::new_v4().to_string(), "topic_active": false}),
            22,
        ),
        (
            ReviewEventKind::AiDecision,
            json!({"cycle_id": Uuid::new_v4().to_string(), "topic_active": "true"}),
            23,
        ),
        (
            ReviewEventKind::AiDecision,
            json!({"cycle_id": Uuid::new_v4().to_string()}),
            24,
        ),
    ] {
        let mut inactive = event(
            session_id,
            "activity",
            event_kind,
            started_at + chrono::Duration::minutes(minute),
            Uuid::new_v4(),
        );
        inactive.metadata = metadata;
        store.append_event(inactive).await.unwrap();
    }

    let trigger_at = started_at + chrono::Duration::minutes(27);
    let sessions = store.active_sessions(trigger_at).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].last_activity_at, decision_at);

    store
        .record_trigger(&input("activity", "activity-2", trigger_at))
        .await
        .unwrap()
        .unwrap();
    let reused_session: Uuid = sqlx::query_scalar(
        "SELECT review_session_id
           FROM twitch_crew_review_events
          WHERE source_message_id = 'activity-2'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reused_session, session_id);
}

#[tokio::test]
async fn trigger_und_aktivitaets_append_erzeugen_nie_zwei_aktive_sessions() {
    let Some(pool) = test_pool("crew_review_session_race").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let started_at = Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap();
    let first_cycle = store
        .record_trigger(&input("session-race", "race-1", started_at))
        .await
        .unwrap()
        .unwrap();
    let old_session_id: Uuid = sqlx::query_scalar(
        "SELECT review_session_id
           FROM twitch_crew_review_events
          WHERE metadata->>'cycle_id' = $1
            AND event_kind = 'ricky_message'",
    )
    .bind(first_cycle.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();

    sqlx::raw_sql(
        "CREATE FUNCTION block_new_review_session() RETURNS trigger AS $$
         BEGIN
             PERFORM pg_advisory_xact_lock(2026071703);
             RETURN NEW;
         END
         $$ LANGUAGE plpgsql;
         CREATE TRIGGER block_new_review_session
         BEFORE INSERT ON twitch_crew_review_events
         FOR EACH ROW WHEN (NEW.event_kind = 'session_started')
         EXECUTE FUNCTION block_new_review_session();",
    )
    .execute(&pool)
    .await
    .unwrap();

    let mut gate = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(2026071703)")
        .execute(&mut *gate)
        .await
        .unwrap();

    let trigger_at = started_at + chrono::Duration::minutes(11);
    let trigger_store = store.clone();
    let trigger_task = tokio::spawn(async move {
        trigger_store
            .record_trigger(&input("session-race", "race-2", trigger_at))
            .await
    });
    wait_for_advisory_query(&pool, "INSERT INTO twitch_crew_review_events").await;

    let mut activity = event(
        old_session_id,
        "session-race",
        ReviewEventKind::StreamerTranscript,
        started_at + chrono::Duration::minutes(9),
        Uuid::new_v4(),
    );
    activity.metadata["subject_mentioned"] = json!(true);
    let append_store = store.clone();
    let mut append_task = tokio::spawn(async move { append_store.append_event(activity).await });
    let append_result = tokio::select! {
        result = &mut append_task => Some(result.unwrap()),
        () = wait_for_advisory_query(&pool, "SELECT pg_advisory_xact_lock") => None,
    };

    gate.rollback().await.unwrap();
    let new_cycle = trigger_task.await.unwrap().unwrap().unwrap();
    let append_result = match append_result {
        Some(result) => result,
        None => append_task.await.unwrap(),
    };
    assert!(
        append_result.is_err(),
        "stale activity append must not bypass the channel lock"
    );

    let sessions = store.active_sessions(trigger_at).await.unwrap();
    assert_eq!(sessions.len(), 1);

    let new_session_id: Uuid = sqlx::query_scalar(
        "SELECT review_session_id
           FROM twitch_crew_review_events
          WHERE metadata->>'cycle_id' = $1
            AND event_kind = 'ricky_message'",
    )
    .bind(new_cycle.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    let mut current_activity = event(
        new_session_id,
        "session-race",
        ReviewEventKind::StreamerTranscript,
        trigger_at + chrono::Duration::minutes(1),
        Uuid::new_v4(),
    );
    current_activity.metadata["subject_mentioned"] = json!(true);
    store.append_event(current_activity).await.unwrap();
}

#[tokio::test]
async fn beendete_neueste_session_laesst_keine_aeltere_wiederauferstehen() {
    let Some(pool) = test_pool("crew_review_latest_session_by_id").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let old_session_id = seed_session(&pool, "latest-session", now).await;
    let latest_session_id =
        seed_session(&pool, "latest-session", now - chrono::Duration::days(1)).await;
    sqlx::query(
        "INSERT INTO twitch_crew_review_events (
            review_session_id, channel_login, subject_twitch_user_id,
            event_kind, occurred_at, metadata, expires_at
         ) VALUES ($1, $2, $3, 'session_ended', $4, $5, $4 + INTERVAL '6 months')",
    )
    .bind(latest_session_id)
    .bind("latest-session")
    .bind(RICKY_TWITCH_USER_ID)
    .bind(now - chrono::Duration::hours(23))
    .bind(json!({"cycle_id": Uuid::new_v4().to_string()}))
    .execute(&pool)
    .await
    .unwrap();

    let mut transcript = event(
        old_session_id,
        "latest-session",
        ReviewEventKind::StreamerTranscript,
        now + chrono::Duration::minutes(1),
        Uuid::new_v4(),
    );
    transcript.metadata["subject_mentioned"] = json!(true);
    assert!(matches!(
        store.append_event(transcript).await,
        Err(StoreError::StaleSession)
    ));
    assert!(matches!(
        store
            .append_event(event(
                old_session_id,
                "latest-session",
                ReviewEventKind::RickyMessage,
                now + chrono::Duration::minutes(2),
                Uuid::new_v4(),
            ))
            .await,
        Err(StoreError::StaleSession)
    ));
    assert!(store.active_sessions(now).await.unwrap().is_empty());
}

#[tokio::test]
async fn append_event_akzeptiert_keinen_erfundenen_session_kanal() {
    let Some(pool) = test_pool("crew_review_session_identity").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "stored-channel", now).await;
    let forged = event(
        session_id,
        "invented-channel",
        ReviewEventKind::AiDraft,
        now,
        Uuid::new_v4(),
    );

    assert!(store.append_event(forged).await.is_err());
    let forged_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
           FROM twitch_crew_review_events
          WHERE channel_login = 'invented-channel'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(forged_rows, 0);
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
    let session_id = seed_session(&pool, "tombstone", now).await;
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
    sqlx::query("UPDATE twitch_crew_review_events SET discord_message_id = $1 WHERE id = $2")
        .bind("discord-tombstone")
        .bind(event_id)
        .execute(&pool)
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
    let session_id = seed_session(&pool, "cleanup", Utc::now()).await;
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
    sqlx::query(
        "UPDATE twitch_crew_review_events
            SET discord_message_id = 'discord-mixed'
          WHERE id = ANY($1::bigint[])",
    )
    .bind(vec![expired_id, fresh_id])
    .execute(&pool)
    .await
    .unwrap();

    assert!(store
        .expired_discord_groups(Utc::now() + chrono::Duration::days(3_650), 10)
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
    let groups = store
        .expired_discord_groups(Utc.timestamp_opt(0, 0).unwrap(), 10)
        .await
        .unwrap();
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
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "pending", now).await;
    let pending_cycle = Uuid::new_v4();
    let completed_cycle = Uuid::new_v4();

    let completed_input = store
        .append_event(event(
            session_id,
            "pending",
            ReviewEventKind::RickyMessage,
            now,
            completed_cycle,
        ))
        .await
        .unwrap();
    let completed_claim = store
        .pending_model_inputs(session_id)
        .await
        .unwrap()
        .unwrap();
    store
        .append_claimed_model_event(
            completed_claim.claim_id,
            event(
                session_id,
                "pending",
                ReviewEventKind::AiDecision,
                now + chrono::Duration::seconds(1),
                completed_cycle,
            ),
        )
        .await
        .unwrap();
    let pending_ricky = store
        .append_event(event(
            session_id,
            "pending",
            ReviewEventKind::RickyMessage,
            now + chrono::Duration::seconds(2),
            pending_cycle,
        ))
        .await
        .unwrap();
    let pending_transcript = store
        .append_event(event(
            session_id,
            "pending",
            ReviewEventKind::StreamerTranscript,
            now + chrono::Duration::seconds(3),
            pending_cycle,
        ))
        .await
        .unwrap();

    let model_inputs = store
        .pending_model_inputs(session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        model_inputs
            .events
            .iter()
            .map(|event| event.id)
            .collect::<Vec<_>>(),
        vec![pending_ricky, pending_transcript]
    );
    assert!(model_inputs.claim_until < model_inputs.events[0].expires_at);

    sqlx::query(
        "UPDATE twitch_crew_review_events
            SET discord_message_id = 'discord-posted'
          WHERE id = ANY($1::bigint[])",
    )
    .bind(vec![pending_ricky, completed_input])
    .execute(&pool)
    .await
    .unwrap();
    assert!(store.pending_discord_cycles(10).await.unwrap().is_empty());
}

#[tokio::test]
async fn abgelaufene_events_verlassen_keine_pending_queue() {
    let Some(pool) = test_pool("crew_review_expired_queues").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let expired_at = Utc::now() - chrono::Duration::days(200);
    let session_id = seed_session(&pool, "expired-queues", expired_at).await;
    let model_cycle_id = Uuid::new_v4();
    store
        .append_event(event(
            session_id,
            "expired-queues",
            ReviewEventKind::RickyMessage,
            expired_at,
            model_cycle_id,
        ))
        .await
        .unwrap();
    let discord_cycle_id = Uuid::new_v4();
    store
        .append_event(event(
            session_id,
            "expired-queues",
            ReviewEventKind::ProviderError,
            expired_at + chrono::Duration::seconds(1),
            discord_cycle_id,
        ))
        .await
        .unwrap();

    assert!(store
        .pending_model_inputs(session_id)
        .await
        .unwrap()
        .is_none());
    assert!(store.pending_discord_cycles(10).await.unwrap().is_empty());
}

#[tokio::test]
async fn model_inputs_werden_atomar_geclaimt_und_nach_timeout_freigegeben() {
    let Some(pool) = test_pool("crew_review_model_claim").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "model-claim", now).await;
    let cycle_id = Uuid::new_v4();
    let event_id = store
        .append_event(event(
            session_id,
            "model-claim",
            ReviewEventKind::StreamerTranscript,
            now,
            cycle_id,
        ))
        .await
        .unwrap();

    let (left, right) = tokio::join!(
        store.pending_model_inputs(session_id),
        store.pending_model_inputs(session_id)
    );
    let claims = [left.unwrap(), right.unwrap()];
    assert_eq!(claims.iter().filter(|claim| claim.is_some()).count(), 1);
    let first = claims.into_iter().flatten().next().unwrap();
    assert_eq!(
        first
            .events
            .iter()
            .map(|event| event.id)
            .collect::<Vec<_>>(),
        vec![event_id]
    );
    assert!(first.claim_until < first.events[0].expires_at);

    sqlx::query(
        "UPDATE twitch_crew_review_events
            SET model_claim_until = NOW() - INTERVAL '1 second'
          WHERE model_claim_id = $1",
    )
    .bind(first.claim_id)
    .execute(&pool)
    .await
    .unwrap();
    let retried = store
        .pending_model_inputs(session_id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(retried.claim_id, first.claim_id);
    assert_eq!(retried.events[0].id, event_id);

    store
        .append_claimed_model_event(
            retried.claim_id,
            event(
                session_id,
                "model-claim",
                ReviewEventKind::ProviderError,
                now + chrono::Duration::seconds(1),
                cycle_id,
            ),
        )
        .await
        .unwrap();
    assert!(store
        .pending_model_inputs(session_id)
        .await
        .unwrap()
        .is_none());
    let remaining_claims: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
           FROM twitch_crew_review_events
          WHERE model_claim_id IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining_claims, 0);
}

#[tokio::test]
async fn modellcycle_wird_trotz_konkurrierender_chunk_locks_nur_einmal_geclaimt() {
    let Some(pool) = test_pool("crew_review_model_cycle_claim").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "model-cycle-claim", now).await;
    let cycle_id = Uuid::new_v4();
    let mut chunked = event(
        session_id,
        "model-cycle-claim",
        ReviewEventKind::StreamerTranscript,
        now,
        cycle_id,
    );
    chunked.content = Some("wort ".repeat(260));
    store.append_event(chunked).await.unwrap();
    let input_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT id
           FROM twitch_crew_review_events
          WHERE review_session_id = $1
            AND metadata->>'cycle_id' = $2
            AND event_kind = 'streamer_transcript'
          ORDER BY id",
    )
    .bind(session_id)
    .bind(cycle_id.to_string())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(input_ids.len(), 2);

    let mut gate = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM twitch_crew_review_events WHERE id = $1 FOR UPDATE")
        .bind(input_ids[1])
        .execute(&mut *gate)
        .await
        .unwrap();

    let first_store = store.clone();
    let mut first_task =
        tokio::spawn(async move { first_store.pending_model_inputs(session_id).await });
    let early_first = tokio::select! {
        result = &mut first_task => Some(result.unwrap()),
        () = wait_for_lock_query(&pool, "WITH eligible_cycles AS") => None,
    };
    assert!(
        early_first.is_none(),
        "erster Worker hat einen Teilcycle geclaimt: {early_first:?}"
    );

    let second_store = store.clone();
    let mut second_task =
        tokio::spawn(async move { second_store.pending_model_inputs(session_id).await });
    let early_second = tokio::select! {
        result = &mut second_task => Some(result.unwrap()),
        () = wait_for_advisory_query(&pool, "pg_advisory_xact_lock") => None,
    };
    assert!(
        early_second.is_none(),
        "zweiter Worker hat den gesperrten Cycle konkurrierend geclaimt: {early_second:?}"
    );

    gate.rollback().await.unwrap();
    let claims = [
        first_task.await.unwrap().unwrap(),
        second_task.await.unwrap().unwrap(),
    ];
    assert_eq!(claims.iter().filter(|claim| claim.is_some()).count(), 1);
    let claimed_ids = claims
        .into_iter()
        .flatten()
        .next()
        .unwrap()
        .events
        .into_iter()
        .map(|event| event.id)
        .collect::<Vec<_>>();
    assert_eq!(claimed_ids, input_ids);
}

#[tokio::test]
async fn grosser_frischer_modellcycle_wird_ohne_tabellenstatistik_schnell_geclaimt() {
    let Some(pool) = test_pool("crew_review_large_fresh_cycle_claim").await else {
        return;
    };
    sqlx::query(
        "ALTER TABLE twitch_crew_review_events SET (
            autovacuum_enabled = false,
            toast.autovacuum_enabled = false
        )",
    )
    .execute(&pool)
    .await
    .unwrap();

    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "large-fresh-cycle", now).await;
    let cycle_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO twitch_crew_review_events (
            review_session_id, channel_login, subject_twitch_user_id,
            event_kind, occurred_at, content, metadata, expires_at
         )
         SELECT $1, 'large-fresh-cycle', $2, 'streamer_transcript',
                $3 + input_no * INTERVAL '1 microsecond', 'chunk',
                jsonb_build_object('cycle_id', $4::text),
                $3 + INTERVAL '6 months'
           FROM generate_series(1, 30001) AS inputs(input_no)",
    )
    .bind(session_id)
    .bind(RICKY_TWITCH_USER_ID)
    .bind(now)
    .bind(cycle_id)
    .execute(&pool)
    .await
    .unwrap();
    let was_analyzed: bool = sqlx::query_scalar(
        "SELECT last_analyze IS NOT NULL OR last_autoanalyze IS NOT NULL
           FROM pg_stat_user_tables
          WHERE relid = 'twitch_crew_review_events'::regclass",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        !was_analyzed,
        "frische Testtabelle darf keine Statistik haben"
    );

    let started = Instant::now();
    let first = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        store.pending_model_inputs(session_id),
    )
    .await
    .expect("30.001 frische Inputs muessen innerhalb von 15s claimbar sein")
    .unwrap()
    .unwrap();
    let elapsed = started.elapsed();
    eprintln!("30.001 frische Inputs geclaimt in {elapsed:?}");
    assert_eq!(first.events.len(), 30_001);

    let second = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        store.pending_model_inputs(session_id),
    )
    .await
    .expect("zweiter Claim muss innerhalb von 5s abgeschlossen sein")
    .unwrap();
    assert!(second.is_none());
}

#[tokio::test]
async fn authentifizierter_modellabschluss_ueberlebt_sessionende_und_reclaim() {
    let Some(pool) = test_pool("crew_review_model_completion_after_end").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();

    let first_session = seed_session(&pool, "completion-before-end", now).await;
    let first_cycle = Uuid::new_v4();
    store
        .append_event(event(
            first_session,
            "completion-before-end",
            ReviewEventKind::RickyMessage,
            now,
            first_cycle,
        ))
        .await
        .unwrap();
    let first_claim = store
        .pending_model_inputs(first_session)
        .await
        .unwrap()
        .unwrap();
    let mut first_decision = event(
        first_session,
        "completion-before-end",
        ReviewEventKind::AiDecision,
        now + chrono::Duration::seconds(1),
        first_cycle,
    );
    first_decision.metadata["topic_active"] = json!(true);
    store
        .append_claimed_model_event(first_claim.claim_id, first_decision)
        .await
        .unwrap();
    store
        .append_event(event(
            first_session,
            "completion-before-end",
            ReviewEventKind::SessionEnded,
            now + chrono::Duration::seconds(2),
            Uuid::new_v4(),
        ))
        .await
        .unwrap();

    let second_session = seed_session(&pool, "completion-after-end", now).await;
    let second_cycle = Uuid::new_v4();
    store
        .append_event(event(
            second_session,
            "completion-after-end",
            ReviewEventKind::RickyMessage,
            now,
            second_cycle,
        ))
        .await
        .unwrap();
    let second_claim = store
        .pending_model_inputs(second_session)
        .await
        .unwrap()
        .unwrap();
    store
        .append_event(event(
            second_session,
            "completion-after-end",
            ReviewEventKind::SessionEnded,
            now + chrono::Duration::seconds(1),
            Uuid::new_v4(),
        ))
        .await
        .unwrap();
    let mut second_decision = event(
        second_session,
        "completion-after-end",
        ReviewEventKind::AiDecision,
        now + chrono::Duration::seconds(2),
        second_cycle,
    );
    second_decision.metadata["topic_active"] = json!(true);
    store
        .append_claimed_model_event(second_claim.claim_id, second_decision)
        .await
        .unwrap();

    let reclaim_session = seed_session(&pool, "completion-reclaim-after-end", now).await;
    let reclaim_cycle = Uuid::new_v4();
    store
        .append_event(event(
            reclaim_session,
            "completion-reclaim-after-end",
            ReviewEventKind::StreamerTranscript,
            now,
            reclaim_cycle,
        ))
        .await
        .unwrap();
    let stale_claim = store
        .pending_model_inputs(reclaim_session)
        .await
        .unwrap()
        .unwrap();
    store
        .append_event(event(
            reclaim_session,
            "completion-reclaim-after-end",
            ReviewEventKind::SessionEnded,
            now + chrono::Duration::seconds(1),
            Uuid::new_v4(),
        ))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE twitch_crew_review_events
            SET model_claim_until = NOW() - INTERVAL '1 second'
          WHERE model_claim_id = $1",
    )
    .bind(stale_claim.claim_id)
    .execute(&pool)
    .await
    .unwrap();
    let reclaimed = store
        .pending_model_inputs(reclaim_session)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(reclaimed.claim_id, stale_claim.claim_id);
    let mut reclaimed_decision = event(
        reclaim_session,
        "completion-reclaim-after-end",
        ReviewEventKind::AiDecision,
        now + chrono::Duration::seconds(2),
        reclaim_cycle,
    );
    reclaimed_decision.metadata["topic_active"] = json!(true);
    store
        .append_claimed_model_event(reclaimed.claim_id, reclaimed_decision)
        .await
        .unwrap();

    assert!(store.active_sessions(Utc::now()).await.unwrap().is_empty());
    let session_starts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_crew_review_events WHERE event_kind = 'session_started'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(session_starts, 3);
}

#[tokio::test]
async fn modellabschluss_akzeptiert_nur_den_aktiven_claim_desselben_cycles() {
    let Some(pool) = test_pool("crew_review_model_completion_claim").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "model-completion", now).await;
    let other_session_id = seed_session(&pool, "model-completion-other", now).await;
    let cycle_id = Uuid::new_v4();
    let target_input_id = store
        .append_event(event(
            session_id,
            "model-completion",
            ReviewEventKind::RickyMessage,
            now,
            cycle_id,
        ))
        .await
        .unwrap();
    let other_cycle_id = Uuid::new_v4();
    let other_input_id = store
        .append_event(event(
            session_id,
            "model-completion",
            ReviewEventKind::StreamerTranscript,
            now + chrono::Duration::milliseconds(100),
            other_cycle_id,
        ))
        .await
        .unwrap();

    let claim_a = store
        .pending_model_inputs(session_id)
        .await
        .unwrap()
        .unwrap();
    sqlx::query(
        "UPDATE twitch_crew_review_events
            SET model_claim_until = NOW() - INTERVAL '1 second'
          WHERE model_claim_id = $1",
    )
    .bind(claim_a.claim_id)
    .execute(&pool)
    .await
    .unwrap();
    let claim_b = store
        .pending_model_inputs(session_id)
        .await
        .unwrap()
        .unwrap();

    let stale = store
        .append_claimed_model_event(
            claim_a.claim_id,
            event(
                session_id,
                "model-completion",
                ReviewEventKind::AiDecision,
                now + chrono::Duration::seconds(1),
                cycle_id,
            ),
        )
        .await;
    assert!(matches!(stale, Err(StoreError::InvalidClaim)));
    let foreign_cycle = store
        .append_claimed_model_event(
            claim_b.claim_id,
            event(
                session_id,
                "model-completion",
                ReviewEventKind::ProviderError,
                now + chrono::Duration::seconds(2),
                Uuid::new_v4(),
            ),
        )
        .await;
    assert!(matches!(foreign_cycle, Err(StoreError::InvalidClaim)));
    let foreign_session = store
        .append_claimed_model_event(
            claim_b.claim_id,
            event(
                other_session_id,
                "model-completion-other",
                ReviewEventKind::ProviderError,
                now + chrono::Duration::seconds(3),
                cycle_id,
            ),
        )
        .await;
    assert!(matches!(foreign_session, Err(StoreError::InvalidClaim)));

    let stale_terminals: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
           FROM twitch_crew_review_events
          WHERE review_session_id = $1
            AND metadata->>'cycle_id' = $2
            AND event_kind IN ('ai_decision', 'provider_error')",
    )
    .bind(session_id)
    .bind(cycle_id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stale_terminals, 0);
    let active_claim: Option<Uuid> = sqlx::query_scalar(
        "SELECT model_claim_id
           FROM twitch_crew_review_events
          WHERE review_session_id = $1
            AND metadata->>'cycle_id' = $2
            AND event_kind = 'ricky_message'",
    )
    .bind(session_id)
    .bind(cycle_id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(active_claim, Some(claim_b.claim_id));

    store
        .append_claimed_model_event(
            claim_b.claim_id,
            event(
                session_id,
                "model-completion",
                ReviewEventKind::AiDraft,
                now + chrono::Duration::seconds(4),
                cycle_id,
            ),
        )
        .await
        .unwrap();
    let active_after_draft: Option<Uuid> = sqlx::query_scalar(
        "SELECT model_claim_id
           FROM twitch_crew_review_events
          WHERE review_session_id = $1
            AND metadata->>'cycle_id' = $2
            AND event_kind = 'ricky_message'",
    )
    .bind(session_id)
    .bind(cycle_id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(active_after_draft, Some(claim_b.claim_id));

    store
        .append_claimed_model_event(
            claim_b.claim_id,
            event(
                session_id,
                "model-completion",
                ReviewEventKind::AiDecision,
                now + chrono::Duration::seconds(5),
                cycle_id,
            ),
        )
        .await
        .unwrap();
    let remaining_claims: Vec<Option<Uuid>> = sqlx::query_scalar(
        "SELECT model_claim_id
           FROM twitch_crew_review_events
          WHERE id = ANY($1::bigint[])
          ORDER BY id",
    )
    .bind(vec![target_input_id, other_input_id])
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(remaining_claims, vec![None, Some(claim_b.claim_id)]);
    assert!(store
        .pending_model_inputs(session_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn normaler_append_umgeht_keinen_modellclaim() {
    let Some(pool) = test_pool("crew_review_model_completion_bypass").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "model-bypass", now).await;
    let cycle_id = Uuid::new_v4();
    store
        .append_event(event(
            session_id,
            "model-bypass",
            ReviewEventKind::StreamerTranscript,
            now,
            cycle_id,
        ))
        .await
        .unwrap();
    let claim = store
        .pending_model_inputs(session_id)
        .await
        .unwrap()
        .unwrap();

    let result = store
        .append_event(event(
            session_id,
            "model-bypass",
            ReviewEventKind::ProviderError,
            now + chrono::Duration::seconds(1),
            cycle_id,
        ))
        .await;
    assert!(matches!(result, Err(StoreError::InvalidClaim)));
    let active_claim: Option<Uuid> = sqlx::query_scalar(
        "SELECT model_claim_id
           FROM twitch_crew_review_events
          WHERE review_session_id = $1
            AND metadata->>'cycle_id' = $2
            AND event_kind = 'streamer_transcript'",
    )
    .bind(session_id)
    .bind(cycle_id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(active_claim, Some(claim.claim_id));

    let audit_cycle = Uuid::new_v4();
    store
        .append_event(event(
            session_id,
            "model-bypass",
            ReviewEventKind::ProviderError,
            now + chrono::Duration::seconds(2),
            audit_cycle,
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn discord_cycles_werden_atomar_geclaimt_und_nach_timeout_freigegeben() {
    let Some(pool) = test_pool("crew_review_discord_claim").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "discord-claim", now).await;
    let cycle_id = Uuid::new_v4();
    let event_id = store
        .append_event(event(
            session_id,
            "discord-claim",
            ReviewEventKind::AiDecision,
            now,
            cycle_id,
        ))
        .await
        .unwrap();

    let (left, right) = tokio::join!(
        store.pending_discord_cycles(10),
        store.pending_discord_cycles(10)
    );
    let mut batches = [left.unwrap(), right.unwrap()];
    assert_eq!(batches.iter().filter(|batch| !batch.is_empty()).count(), 1);
    let first = batches.iter_mut().find_map(|batch| batch.pop()).unwrap();
    assert_eq!(first.events[0].id, event_id);
    assert!(first.claim_until < first.events[0].expires_at);
    assert_eq!(
        store
            .delete_expired_unposted(first.events[0].expires_at - chrono::Duration::microseconds(1))
            .await
            .unwrap(),
        0
    );

    sqlx::query(
        "UPDATE twitch_crew_review_events
            SET discord_claim_until = NOW() - INTERVAL '1 second'
          WHERE discord_claim_id = $1",
    )
    .bind(first.claim_id)
    .execute(&pool)
    .await
    .unwrap();
    let retried = store.pending_discord_cycles(10).await.unwrap();
    assert_eq!(retried.len(), 1);
    assert_ne!(retried[0].claim_id, first.claim_id);
    assert_eq!(retried[0].events[0].id, event_id);
}

#[tokio::test]
async fn discord_claimt_keinen_nur_teilweise_claimbaren_cycle() {
    let Some(pool) = test_pool("crew_review_discord_complete_cycle").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "discord-complete", now).await;

    let expiring_cycle = Uuid::new_v4();
    let expiring_id = store
        .append_event(event(
            session_id,
            "discord-complete",
            ReviewEventKind::ProviderError,
            now,
            expiring_cycle,
        ))
        .await
        .unwrap();
    let fresh_id = store
        .append_event(event(
            session_id,
            "discord-complete",
            ReviewEventKind::AiDraft,
            now + chrono::Duration::seconds(1),
            expiring_cycle,
        ))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE twitch_crew_review_events
            SET expires_at = NOW() + INTERVAL '5 minutes'
          WHERE id = $1",
    )
    .bind(expiring_id)
    .execute(&pool)
    .await
    .unwrap();

    let foreign_cycle = Uuid::new_v4();
    let foreign_id = store
        .append_event(event(
            session_id,
            "discord-complete",
            ReviewEventKind::ProviderError,
            now + chrono::Duration::seconds(2),
            foreign_cycle,
        ))
        .await
        .unwrap();
    let free_id = store
        .append_event(event(
            session_id,
            "discord-complete",
            ReviewEventKind::AiDraft,
            now + chrono::Duration::seconds(3),
            foreign_cycle,
        ))
        .await
        .unwrap();
    let foreign_claim_id = Uuid::new_v4();
    sqlx::query(
        "UPDATE twitch_crew_review_events
            SET discord_claim_id = $1,
                discord_claim_until = NOW() + INTERVAL '4 minutes'
          WHERE id = $2",
    )
    .bind(foreign_claim_id)
    .bind(foreign_id)
    .execute(&pool)
    .await
    .unwrap();

    assert!(store.pending_discord_cycles(10).await.unwrap().is_empty());
    let claims: Vec<Option<Uuid>> = sqlx::query_scalar(
        "SELECT discord_claim_id
           FROM twitch_crew_review_events
          WHERE id = ANY($1::bigint[])
          ORDER BY id",
    )
    .bind(vec![expiring_id, fresh_id, foreign_id, free_id])
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(claims, vec![None, None, Some(foreign_claim_id), None]);
}

#[tokio::test]
async fn discord_claim_gruppiert_ungepostete_zeilen_nach_session_und_cycle() {
    let Some(pool) = test_pool("crew_review_discord_session_cycle").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let cycle_id = Uuid::new_v4();
    let mixed_session = seed_session(&pool, "discord-mixed-cycle", now).await;
    let posted_id = store
        .append_event(event(
            mixed_session,
            "discord-mixed-cycle",
            ReviewEventKind::AiDecision,
            now,
            cycle_id,
        ))
        .await
        .unwrap();
    store
        .append_event(event(
            mixed_session,
            "discord-mixed-cycle",
            ReviewEventKind::AiDraft,
            now + chrono::Duration::seconds(1),
            cycle_id,
        ))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE twitch_crew_review_events
            SET discord_message_id = 'discord-already-posted'
          WHERE id = $1",
    )
    .bind(posted_id)
    .execute(&pool)
    .await
    .unwrap();

    let eligible_session = seed_session(&pool, "discord-eligible-cycle", now).await;
    let eligible_id = store
        .append_event(event(
            eligible_session,
            "discord-eligible-cycle",
            ReviewEventKind::ProviderError,
            now + chrono::Duration::seconds(2),
            cycle_id,
        ))
        .await
        .unwrap();

    let cycles = store.pending_discord_cycles(10).await.unwrap();
    assert_eq!(cycles.len(), 1);
    assert_eq!(cycles[0].session_id, eligible_session);
    assert_eq!(cycles[0].cycle_id, cycle_id);
    assert_eq!(cycles[0].events.len(), 1);
    assert_eq!(cycles[0].events[0].id, eligible_id);
}

#[tokio::test]
async fn discord_claim_und_late_append_versiegeln_den_cycle_atomar() {
    let Some(pool) = test_pool("crew_review_discord_late_append").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "discord-late", now).await;
    let cycle_id = Uuid::new_v4();
    let original_id = store
        .append_event(event(
            session_id,
            "discord-late",
            ReviewEventKind::ProviderError,
            now,
            cycle_id,
        ))
        .await
        .unwrap();

    sqlx::raw_sql(
        "CREATE FUNCTION block_discord_claim() RETURNS trigger AS $$
         BEGIN
             PERFORM pg_advisory_xact_lock(2026071704);
             RETURN NEW;
         END
         $$ LANGUAGE plpgsql;
         CREATE TRIGGER block_discord_claim
         BEFORE UPDATE ON twitch_crew_review_events
         FOR EACH ROW WHEN (
             OLD.discord_claim_id IS NULL AND NEW.discord_claim_id IS NOT NULL
         )
         EXECUTE FUNCTION block_discord_claim();",
    )
    .execute(&pool)
    .await
    .unwrap();
    let mut gate = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(2026071704)")
        .execute(&mut *gate)
        .await
        .unwrap();

    let claim_store = store.clone();
    let mut claim_task = tokio::spawn(async move { claim_store.pending_discord_cycles(1).await });
    let early_claim = tokio::select! {
        result = &mut claim_task => Some(result.unwrap()),
        () = wait_for_advisory_query(&pool, "WITH selected_cycles AS") => None,
    };
    assert!(
        early_claim.is_none(),
        "discord claim returned before barrier: {early_claim:?}"
    );

    let append_store = store.clone();
    let mut append_task = tokio::spawn(async move {
        append_store
            .append_event(event(
                session_id,
                "discord-late",
                ReviewEventKind::AiDraft,
                now + chrono::Duration::seconds(1),
                cycle_id,
            ))
            .await
    });
    let early_append = tokio::select! {
        result = &mut append_task => Some(result.unwrap()),
        () = wait_for_advisory_query(&pool, "hashtextextended($1 || ':' || $2, 1)") => None,
    };

    gate.rollback().await.unwrap();
    let claimed = claim_task.await.unwrap().unwrap();
    let append_result = match early_append {
        Some(result) => result,
        None => append_task.await.unwrap(),
    };
    assert!(matches!(append_result, Err(StoreError::InvalidClaim)));
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].events.len(), 1);
    assert_eq!(claimed[0].events[0].id, original_id);
    let cycle_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
           FROM twitch_crew_review_events
          WHERE review_session_id = $1
            AND metadata->>'cycle_id' = $2",
    )
    .bind(session_id)
    .bind(cycle_id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cycle_rows, 1);
}

#[tokio::test]
async fn fuenf_minuten_restlaufzeit_reichen_fuer_keinen_claim() {
    let Some(pool) = test_pool("crew_review_claim_boundary").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "claim-boundary", now).await;
    let model_id = store
        .append_event(event(
            session_id,
            "claim-boundary",
            ReviewEventKind::RickyMessage,
            now,
            Uuid::new_v4(),
        ))
        .await
        .unwrap();
    let discord_id = store
        .append_event(event(
            session_id,
            "claim-boundary",
            ReviewEventKind::ProviderError,
            now + chrono::Duration::seconds(1),
            Uuid::new_v4(),
        ))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE twitch_crew_review_events
            SET expires_at = NOW() + INTERVAL '5 minutes'
          WHERE id = ANY($1::bigint[])",
    )
    .bind(vec![model_id, discord_id])
    .execute(&pool)
    .await
    .unwrap();

    assert!(store
        .pending_model_inputs(session_id)
        .await
        .unwrap()
        .is_none());
    assert!(store.pending_discord_cycles(10).await.unwrap().is_empty());
    let claims: Vec<(Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        "SELECT model_claim_id, discord_claim_id
           FROM twitch_crew_review_events
          WHERE id = ANY($1::bigint[])
          ORDER BY id",
    )
    .bind(vec![model_id, discord_id])
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(claims, vec![(None, None), (None, None)]);
}

#[tokio::test]
async fn cleanup_und_claim_treffen_sich_nie_ueber_der_loeschfrist() {
    let Some(pool) = test_pool("crew_review_claim_cleanup").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "claim-cleanup", now).await;
    let event_id = store
        .append_event(event(
            session_id,
            "claim-cleanup",
            ReviewEventKind::RickyMessage,
            now,
            Uuid::new_v4(),
        ))
        .await
        .unwrap();
    let expires_at = now + chrono::Duration::minutes(10);
    sqlx::query("UPDATE twitch_crew_review_events SET expires_at = $1 WHERE id = $2")
        .bind(expires_at)
        .bind(event_id)
        .execute(&pool)
        .await
        .unwrap();

    let claim = store
        .pending_model_inputs(session_id)
        .await
        .unwrap()
        .unwrap();
    assert!(claim.claim_until < expires_at);
    assert_eq!(
        store
            .delete_expired_unposted(now + chrono::Duration::days(3_650))
            .await
            .unwrap(),
        0
    );
    sqlx::query(
        "UPDATE twitch_crew_review_events
            SET expires_at = occurred_at,
                model_claim_until = NOW() - INTERVAL '1 second'
          WHERE id = $1",
    )
    .bind(event_id)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        store
            .delete_expired_unposted(Utc.timestamp_opt(0, 0).unwrap())
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn mark_discord_sent_ist_fuer_unvollstaendige_oder_fremde_claims_atomar() {
    let Some(pool) = test_pool("crew_review_mark_claim").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "mark-claim", now).await;
    let cycle_id = Uuid::new_v4();
    let first_id = store
        .append_event(event(
            session_id,
            "mark-claim",
            ReviewEventKind::AiDecision,
            now,
            cycle_id,
        ))
        .await
        .unwrap();
    let second_id = store
        .append_event(event(
            session_id,
            "mark-claim",
            ReviewEventKind::AiDraft,
            now + chrono::Duration::seconds(1),
            cycle_id,
        ))
        .await
        .unwrap();
    let cycle = store
        .pending_discord_cycles(1)
        .await
        .unwrap()
        .pop()
        .unwrap();

    assert!(store
        .mark_discord_sent(&[first_id], cycle.claim_id, "discord-incomplete")
        .await
        .is_err());
    let posted: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT discord_message_id
           FROM twitch_crew_review_events
          WHERE id = ANY($1::bigint[])
          ORDER BY id",
    )
    .bind(vec![first_id, second_id])
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(posted, vec![None, None]);

    assert!(store
        .mark_discord_sent(&[first_id, second_id], Uuid::new_v4(), "discord-foreign")
        .await
        .is_err());
    store
        .mark_discord_sent(&[first_id, second_id], cycle.claim_id, "discord-valid")
        .await
        .unwrap();

    let expired_cycle = Uuid::new_v4();
    let expired_id = store
        .append_event(event(
            session_id,
            "mark-claim",
            ReviewEventKind::ProviderError,
            now + chrono::Duration::seconds(2),
            expired_cycle,
        ))
        .await
        .unwrap();
    let expired_claim = store
        .pending_discord_cycles(1)
        .await
        .unwrap()
        .pop()
        .unwrap();
    sqlx::query(
        "UPDATE twitch_crew_review_events
            SET discord_claim_until = NOW() - INTERVAL '2 seconds',
                expires_at = NOW() - INTERVAL '1 second'
          WHERE id = $1",
    )
    .bind(expired_id)
    .execute(&pool)
    .await
    .unwrap();
    assert!(store
        .mark_discord_sent(&[expired_id], expired_claim.claim_id, "discord-expired")
        .await
        .is_err());
}

#[tokio::test]
async fn vollstaendige_cycles_desselben_discord_batches_lassen_sich_getrennt_markieren() {
    let Some(pool) = test_pool("crew_review_mark_packed_cycles").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "mark-packed", now).await;
    let first_id = store
        .append_event(event(
            session_id,
            "mark-packed",
            ReviewEventKind::ProviderError,
            now,
            Uuid::new_v4(),
        ))
        .await
        .unwrap();
    let second_id = store
        .append_event(event(
            session_id,
            "mark-packed",
            ReviewEventKind::ProviderError,
            now + chrono::Duration::seconds(1),
            Uuid::new_v4(),
        ))
        .await
        .unwrap();
    let cycles = store.pending_discord_cycles(2).await.unwrap();
    assert_eq!(cycles.len(), 2);
    assert_eq!(cycles[0].claim_id, cycles[1].claim_id);

    store
        .mark_discord_sent(&[first_id], cycles[0].claim_id, "discord-first")
        .await
        .unwrap();
    let second_claim: Option<Uuid> =
        sqlx::query_scalar("SELECT discord_claim_id FROM twitch_crew_review_events WHERE id = $1")
            .bind(second_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(second_claim, Some(cycles[1].claim_id));
    store
        .mark_discord_sent(&[second_id], cycles[1].claim_id, "discord-second")
        .await
        .unwrap();
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
    let session_id = seed_session(&pool, "chunks", Utc::now()).await;
    let mut new_event = event(
        session_id,
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
           FROM twitch_crew_review_events
          WHERE event_kind = 'ricky_message'
          ORDER BY id",
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

#[tokio::test]
async fn session_events_liefert_geordnete_sitzungshistorie_ohne_tombstone_inhalt() {
    let Some(pool) = test_pool("crew_review_session_events").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "session-events", now).await;
    let other_session_id = seed_session(&pool, "session-events-other", now).await;
    let occurred_at = now + chrono::Duration::seconds(1);
    let first_id = store
        .append_event(event(
            session_id,
            "session-events",
            ReviewEventKind::AiDraft,
            occurred_at,
            Uuid::new_v4(),
        ))
        .await
        .unwrap();
    let second_id = store
        .append_event(event(
            session_id,
            "session-events",
            ReviewEventKind::ProviderError,
            occurred_at,
            Uuid::new_v4(),
        ))
        .await
        .unwrap();
    store
        .append_event(event(
            other_session_id,
            "session-events-other",
            ReviewEventKind::AiDraft,
            occurred_at,
            Uuid::new_v4(),
        ))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE twitch_crew_review_events
            SET content = NULL,
                metadata = jsonb_build_object('tombstoned_at', NOW()),
                provider = NULL,
                model = NULL,
                confidence = NULL,
                tombstoned_at = NOW()
          WHERE id = $1",
    )
    .bind(first_id)
    .execute(&pool)
    .await
    .unwrap();

    let events = store.session_events(session_id).await.unwrap();

    assert_eq!(events.len(), 3);
    assert_eq!(events[1].id, first_id);
    assert_eq!(events[2].id, second_id);
    assert!(events.iter().all(|event| event.session_id == session_id));
    assert_eq!(events[1].content, None);
    assert!(events[1].metadata.get("cycle_id").is_none());
    assert!(events[1].tombstoned_at.is_some());
}

#[tokio::test]
async fn channel_close_serialisiert_mit_trigger_und_laesst_fremden_kanal_offen() {
    let Some(pool) = test_pool("crew_review_channel_close_race").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "close-race", now).await;
    let foreign_session_id = seed_session(&pool, "close-race-foreign", now).await;
    let mut gate = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind("close-race")
        .execute(&mut *gate)
        .await
        .unwrap();

    let close_store = store.clone();
    let close_task = tokio::spawn(async move {
        close_store
            .close_channel_session(
                "close-race",
                "process_restart",
                now + chrono::Duration::seconds(1),
            )
            .await
    });
    wait_for_advisory_query(&pool, "hashtextextended($1, 0)").await;
    let trigger_store = store.clone();
    let trigger_task = tokio::spawn(async move {
        trigger_store
            .record_trigger(&input(
                "close-race",
                "close-race-after",
                now + chrono::Duration::seconds(2),
            ))
            .await
    });
    gate.rollback().await.unwrap();

    assert!(close_task.await.unwrap().unwrap());
    trigger_task.await.unwrap().unwrap().unwrap();
    let ended: Vec<(Uuid, serde_json::Value)> = sqlx::query_as(
        "SELECT review_session_id, metadata
           FROM twitch_crew_review_events
          WHERE channel_login = 'close-race' AND event_kind = 'session_ended'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].0, session_id);
    assert_eq!(ended[0].1["reason"], "process_restart");
    let open_sessions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
           FROM twitch_crew_review_events started
          WHERE started.channel_login = 'close-race'
            AND started.event_kind = 'session_started'
            AND NOT EXISTS (
                SELECT 1 FROM twitch_crew_review_events ended
                 WHERE ended.review_session_id = started.review_session_id
                   AND ended.event_kind = 'session_ended'
            )",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(open_sessions <= 1);
    let foreign_ended: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM twitch_crew_review_events
             WHERE review_session_id = $1 AND event_kind = 'session_ended'
        )",
    )
    .bind(foreign_session_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!foreign_ended);
    assert!(!store
        .close_channel_session(
            "close-race-foreign-missing",
            "unused",
            now + chrono::Duration::seconds(3),
        )
        .await
        .unwrap());
}

#[tokio::test]
async fn startup_close_beendet_auch_inaktive_sessions_und_ist_idempotent() {
    let Some(pool) = test_pool("crew_review_startup_close").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let inactive_id = seed_session(
        &pool,
        "startup-close-inactive",
        now - chrono::Duration::minutes(11),
    )
    .await;
    let active_id = seed_session(&pool, "startup-close-active", now).await;

    assert_eq!(
        store
            .close_all_open_sessions("process_start", now)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        store
            .close_all_open_sessions("process_start", now)
            .await
            .unwrap(),
        0
    );
    let rows: Vec<(Uuid, serde_json::Value)> = sqlx::query_as(
        "SELECT review_session_id, metadata
           FROM twitch_crew_review_events
          WHERE review_session_id = ANY($1::uuid[])
            AND event_kind = 'session_ended'
          ORDER BY review_session_id",
    )
    .bind(vec![inactive_id, active_id])
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .all(|(_, metadata)| metadata["reason"] == "process_start"));
}

#[tokio::test]
async fn modellabschluss_schreibt_draft_und_genau_ein_terminal_atomar() {
    let Some(pool) = test_pool("crew_review_atomic_model_completion").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "atomic-model", now).await;
    let cycle_id = Uuid::new_v4();
    store
        .append_event(event(
            session_id,
            "atomic-model",
            ReviewEventKind::RickyMessage,
            now,
            cycle_id,
        ))
        .await
        .unwrap();
    let claim = store
        .pending_model_inputs(session_id)
        .await
        .unwrap()
        .unwrap();

    let first_store = store.clone();
    let first = tokio::spawn(async move {
        first_store
            .complete_claimed_model_cycle(
                claim.claim_id,
                Some(event(
                    session_id,
                    "atomic-model",
                    ReviewEventKind::AiDraft,
                    now + chrono::Duration::seconds(1),
                    cycle_id,
                )),
                event(
                    session_id,
                    "atomic-model",
                    ReviewEventKind::AiDecision,
                    now + chrono::Duration::seconds(2),
                    cycle_id,
                ),
            )
            .await
    });
    let second_store = store.clone();
    let second = tokio::spawn(async move {
        second_store
            .complete_claimed_model_cycle(
                claim.claim_id,
                Some(event(
                    session_id,
                    "atomic-model",
                    ReviewEventKind::AiDraft,
                    now + chrono::Duration::seconds(1),
                    cycle_id,
                )),
                event(
                    session_id,
                    "atomic-model",
                    ReviewEventKind::ProviderError,
                    now + chrono::Duration::seconds(2),
                    cycle_id,
                ),
            )
            .await
    });
    let results = [first.await.unwrap(), second.await.unwrap()];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(StoreError::InvalidClaim)))
            .count(),
        1
    );
    let kinds: Vec<String> = sqlx::query_scalar(
        "SELECT event_kind
           FROM twitch_crew_review_events
          WHERE review_session_id = $1
            AND metadata->>'cycle_id' = $2
            AND event_kind IN ('ai_draft', 'ai_decision', 'provider_error')
          ORDER BY id",
    )
    .bind(session_id)
    .bind(cycle_id.to_string())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(kinds.len(), 2);
    assert_eq!(kinds[0], "ai_draft");
    assert!(matches!(
        kinds[1].as_str(),
        "ai_decision" | "provider_error"
    ));
    let remaining_claim: Option<Uuid> = sqlx::query_scalar(
        "SELECT model_claim_id
           FROM twitch_crew_review_events
          WHERE review_session_id = $1
            AND metadata->>'cycle_id' = $2
            AND event_kind = 'ricky_message'",
    )
    .bind(session_id)
    .bind(cycle_id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining_claim, None);
    let repeated = store
        .complete_claimed_model_cycle(
            claim.claim_id,
            None,
            event(
                session_id,
                "atomic-model",
                ReviewEventKind::AiDecision,
                now + chrono::Duration::seconds(3),
                cycle_id,
            ),
        )
        .await;
    assert!(matches!(repeated, Err(StoreError::InvalidClaim)));
}

#[tokio::test]
async fn modellabschluss_lehnt_falschen_claim_lease_session_und_cycle_ohne_schreibzugriff_ab() {
    let Some(pool) = test_pool("crew_review_model_completion_fail_closed").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "model-fail-closed", now).await;
    let other_session_id = seed_session(&pool, "model-fail-closed-other", now).await;
    let cycle_id = Uuid::new_v4();
    store
        .append_event(event(
            session_id,
            "model-fail-closed",
            ReviewEventKind::StreamerTranscript,
            now,
            cycle_id,
        ))
        .await
        .unwrap();
    let claim = store
        .pending_model_inputs(session_id)
        .await
        .unwrap()
        .unwrap();

    let attempts = [
        store
            .complete_claimed_model_cycle(
                Uuid::new_v4(),
                None,
                event(
                    session_id,
                    "model-fail-closed",
                    ReviewEventKind::AiDecision,
                    now,
                    cycle_id,
                ),
            )
            .await,
        store
            .complete_claimed_model_cycle(
                claim.claim_id,
                None,
                event(
                    session_id,
                    "model-fail-closed",
                    ReviewEventKind::ProviderError,
                    now,
                    Uuid::new_v4(),
                ),
            )
            .await,
        store
            .complete_claimed_model_cycle(
                claim.claim_id,
                None,
                event(
                    other_session_id,
                    "model-fail-closed-other",
                    ReviewEventKind::ProviderError,
                    now,
                    cycle_id,
                ),
            )
            .await,
    ];
    assert!(attempts
        .iter()
        .all(|result| matches!(result, Err(StoreError::InvalidClaim))));
    let active_claim: Option<Uuid> = sqlx::query_scalar(
        "SELECT model_claim_id
           FROM twitch_crew_review_events
          WHERE review_session_id = $1 AND event_kind = 'streamer_transcript'",
    )
    .bind(session_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(active_claim, Some(claim.claim_id));

    sqlx::query(
        "UPDATE twitch_crew_review_events
            SET model_claim_until = NOW() - INTERVAL '1 second'
          WHERE model_claim_id = $1",
    )
    .bind(claim.claim_id)
    .execute(&pool)
    .await
    .unwrap();
    let expired = store
        .complete_claimed_model_cycle(
            claim.claim_id,
            Some(event(
                session_id,
                "model-fail-closed",
                ReviewEventKind::AiDraft,
                now,
                cycle_id,
            )),
            event(
                session_id,
                "model-fail-closed",
                ReviewEventKind::AiDecision,
                now,
                cycle_id,
            ),
        )
        .await;
    assert!(matches!(expired, Err(StoreError::InvalidClaim)));
    let generated: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
           FROM twitch_crew_review_events
          WHERE review_session_id = $1
            AND event_kind IN ('ai_draft', 'ai_decision', 'provider_error')",
    )
    .bind(session_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(generated, 0);
}

#[tokio::test]
async fn modellabschluss_sql_fehler_rollt_draft_zurueck_und_behaelt_claim() {
    let Some(pool) = test_pool("crew_review_model_completion_rollback").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "model-rollback", now).await;
    let cycle_id = Uuid::new_v4();
    store
        .append_event(event(
            session_id,
            "model-rollback",
            ReviewEventKind::RickyMessage,
            now,
            cycle_id,
        ))
        .await
        .unwrap();
    let claim = store
        .pending_model_inputs(session_id)
        .await
        .unwrap()
        .unwrap();
    sqlx::raw_sql(
        "CREATE FUNCTION fail_atomic_model_terminal() RETURNS trigger AS $$
         BEGIN
           RAISE EXCEPTION 'forced model terminal failure';
         END;
         $$ LANGUAGE plpgsql;
         CREATE TRIGGER fail_atomic_model_terminal
         BEFORE INSERT ON twitch_crew_review_events
         FOR EACH ROW WHEN (NEW.event_kind = 'ai_decision')
         EXECUTE FUNCTION fail_atomic_model_terminal();",
    )
    .execute(&pool)
    .await
    .unwrap();

    let result = store
        .complete_claimed_model_cycle(
            claim.claim_id,
            Some(event(
                session_id,
                "model-rollback",
                ReviewEventKind::AiDraft,
                now + chrono::Duration::seconds(1),
                cycle_id,
            )),
            event(
                session_id,
                "model-rollback",
                ReviewEventKind::AiDecision,
                now + chrono::Duration::seconds(2),
                cycle_id,
            ),
        )
        .await;
    assert!(matches!(result, Err(StoreError::Database(_))));
    let generated: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
           FROM twitch_crew_review_events
          WHERE review_session_id = $1
            AND metadata->>'cycle_id' = $2
            AND event_kind IN ('ai_draft', 'ai_decision')",
    )
    .bind(session_id)
    .bind(cycle_id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(generated, 0);
    let active_claim: Option<Uuid> = sqlx::query_scalar(
        "SELECT model_claim_id
           FROM twitch_crew_review_events
          WHERE review_session_id = $1 AND event_kind = 'ricky_message'",
    )
    .bind(session_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(active_claim, Some(claim.claim_id));
}

#[tokio::test]
async fn discord_mehrkarten_markierung_persistiert_vollstaendig_und_expired_getrennt() {
    let Some(pool) = test_pool("crew_review_discord_cards").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "discord-cards", now).await;
    let cycle_id = Uuid::new_v4();
    let first_id = store
        .append_event(event(
            session_id,
            "discord-cards",
            ReviewEventKind::AiDraft,
            now,
            cycle_id,
        ))
        .await
        .unwrap();
    let second_id = store
        .append_event(event(
            session_id,
            "discord-cards",
            ReviewEventKind::ProviderError,
            now + chrono::Duration::seconds(1),
            cycle_id,
        ))
        .await
        .unwrap();
    let cycle = store.pending_discord_cycles(1).await.unwrap().remove(0);

    store
        .mark_discord_cards_sent(
            &[
                DiscordCard {
                    event_ids: vec![first_id],
                    message_id: "discord-card-first".to_owned(),
                },
                DiscordCard {
                    event_ids: vec![second_id],
                    message_id: "discord-card-second".to_owned(),
                },
            ],
            cycle.claim_id,
        )
        .await
        .unwrap();
    let posted: Vec<(i64, Option<String>, Option<Uuid>)> = sqlx::query_as(
        "SELECT id, discord_message_id, discord_claim_id
           FROM twitch_crew_review_events
          WHERE id = ANY($1::bigint[])
          ORDER BY id",
    )
    .bind(vec![first_id, second_id])
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        posted,
        vec![
            (first_id, Some("discord-card-first".to_owned()), None),
            (second_id, Some("discord-card-second".to_owned()), None),
        ]
    );
    assert!(store.pending_discord_cycles(1).await.unwrap().is_empty());

    sqlx::query(
        "UPDATE twitch_crew_review_events
            SET expires_at = NOW() - INTERVAL '1 second'
          WHERE id = ANY($1::bigint[])",
    )
    .bind(vec![first_id, second_id])
    .execute(&pool)
    .await
    .unwrap();
    let groups = store.expired_discord_groups(now, 10).await.unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].event_ids.len(), 1);
    assert_eq!(groups[1].event_ids.len(), 1);
    assert_eq!(
        groups
            .iter()
            .map(|group| group.discord_message_id.as_str())
            .collect::<std::collections::HashSet<_>>(),
        std::collections::HashSet::from(["discord-card-first", "discord-card-second"])
    );
}

#[tokio::test]
async fn discord_mehrkarten_markierung_rollt_ungueltige_mengen_vollstaendig_zurueck() {
    let Some(pool) = test_pool("crew_review_discord_cards_invalid").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let session_id = seed_session(&pool, "discord-cards-invalid", now).await;
    let cycle_id = Uuid::new_v4();
    let first_id = store
        .append_event(event(
            session_id,
            "discord-cards-invalid",
            ReviewEventKind::AiDraft,
            now,
            cycle_id,
        ))
        .await
        .unwrap();
    let second_id = store
        .append_event(event(
            session_id,
            "discord-cards-invalid",
            ReviewEventKind::AiDecision,
            now + chrono::Duration::seconds(1),
            cycle_id,
        ))
        .await
        .unwrap();
    let foreign_id = store
        .append_event(event(
            session_id,
            "discord-cards-invalid",
            ReviewEventKind::AiDraft,
            now + chrono::Duration::seconds(2),
            Uuid::new_v4(),
        ))
        .await
        .unwrap();
    let cycle = store.pending_discord_cycles(1).await.unwrap().remove(0);
    let invalid_cards = vec![
        vec![DiscordCard {
            event_ids: vec![first_id],
            message_id: "missing".to_owned(),
        }],
        vec![DiscordCard {
            event_ids: vec![first_id, first_id, second_id],
            message_id: "duplicate".to_owned(),
        }],
        vec![
            DiscordCard {
                event_ids: vec![first_id],
                message_id: "overlap-first".to_owned(),
            },
            DiscordCard {
                event_ids: vec![first_id, second_id],
                message_id: "overlap-second".to_owned(),
            },
        ],
        vec![DiscordCard {
            event_ids: vec![first_id, second_id, foreign_id],
            message_id: "foreign".to_owned(),
        }],
        vec![DiscordCard {
            event_ids: Vec::new(),
            message_id: "empty-events".to_owned(),
        }],
        vec![DiscordCard {
            event_ids: vec![first_id, second_id],
            message_id: "  ".to_owned(),
        }],
    ];
    for cards in invalid_cards {
        let result = store.mark_discord_cards_sent(&cards, cycle.claim_id).await;
        assert!(matches!(result, Err(StoreError::InvalidClaim)));
        let rows: Vec<(Option<String>, Option<Uuid>)> = sqlx::query_as(
            "SELECT discord_message_id, discord_claim_id
               FROM twitch_crew_review_events
              WHERE id = ANY($1::bigint[])
              ORDER BY id",
        )
        .bind(vec![first_id, second_id])
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![(None, Some(cycle.claim_id)), (None, Some(cycle.claim_id))]
        );
    }

    store
        .mark_discord_cards_sent(
            &[
                DiscordCard {
                    event_ids: vec![first_id],
                    message_id: "retry-first".to_owned(),
                },
                DiscordCard {
                    event_ids: vec![second_id],
                    message_id: "retry-second".to_owned(),
                },
            ],
            cycle.claim_id,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn modellabschluss_schreibt_nach_lease_ablauf_im_advisory_lock_nichts() {
    let Some(pool) = test_pool("crew_review_model_lease_lock_wait").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let channel = "model-lease-lock-wait";
    let session_id = seed_session(&pool, channel, now).await;
    let cycle_id = Uuid::new_v4();
    store
        .append_event(event(
            session_id,
            channel,
            ReviewEventKind::RickyMessage,
            now,
            cycle_id,
        ))
        .await
        .unwrap();
    let claim = store
        .pending_model_inputs(session_id)
        .await
        .unwrap()
        .unwrap();
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(channel)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let completion_store = store.clone();
    let completion = tokio::spawn(async move {
        completion_store
            .complete_claimed_model_cycle(
                claim.claim_id,
                Some(event(
                    session_id,
                    channel,
                    ReviewEventKind::AiDraft,
                    now + chrono::Duration::seconds(1),
                    cycle_id,
                )),
                event(
                    session_id,
                    channel,
                    ReviewEventKind::AiDecision,
                    now + chrono::Duration::seconds(2),
                    cycle_id,
                ),
            )
            .await
    });
    wait_for_advisory_query(&pool, "hashtextextended($1, 0)").await;
    let active_during_wait: bool = sqlx::query_scalar(
        "UPDATE twitch_crew_review_events
            SET model_claim_until = clock_timestamp() + INTERVAL '200 milliseconds'
          WHERE model_claim_id = $1
        RETURNING model_claim_until > clock_timestamp()",
    )
    .bind(claim.claim_id)
    .fetch_one(&mut *blocker)
    .await
    .unwrap();
    assert!(active_during_wait);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let expired_during_wait: bool = sqlx::query_scalar(
        "SELECT BOOL_AND(model_claim_until <= clock_timestamp())
           FROM twitch_crew_review_events
          WHERE model_claim_id = $1",
    )
    .bind(claim.claim_id)
    .fetch_one(&mut *blocker)
    .await
    .unwrap();
    assert!(expired_during_wait);
    blocker.commit().await.unwrap();

    let result = completion.await.unwrap();
    assert!(matches!(result, Err(StoreError::InvalidClaim)));
    let generated: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
           FROM twitch_crew_review_events
          WHERE review_session_id = $1
            AND metadata->>'cycle_id' = $2
            AND event_kind IN ('ai_draft', 'ai_decision', 'provider_error')",
    )
    .bind(session_id)
    .bind(cycle_id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(generated, 0);
    let retained_claim: Option<Uuid> = sqlx::query_scalar(
        "SELECT model_claim_id
           FROM twitch_crew_review_events
          WHERE review_session_id = $1
            AND metadata->>'cycle_id' = $2
            AND event_kind = 'ricky_message'",
    )
    .bind(session_id)
    .bind(cycle_id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(retained_claim, Some(claim.claim_id));
    let retried = store
        .pending_model_inputs(session_id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(retried.claim_id, claim.claim_id);
}

#[tokio::test]
async fn discord_markierung_schreibt_nach_lease_ablauf_im_row_lock_nichts() {
    let Some(pool) = test_pool("crew_review_discord_lease_lock_wait").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let channel = "discord-lease-lock-wait";
    let session_id = seed_session(&pool, channel, now).await;
    let cycle_id = Uuid::new_v4();
    let first_id = store
        .append_event(event(
            session_id,
            channel,
            ReviewEventKind::AiDraft,
            now,
            cycle_id,
        ))
        .await
        .unwrap();
    let second_id = store
        .append_event(event(
            session_id,
            channel,
            ReviewEventKind::ProviderError,
            now + chrono::Duration::seconds(1),
            cycle_id,
        ))
        .await
        .unwrap();
    let cycle = store.pending_discord_cycles(1).await.unwrap().remove(0);
    let event_ids = vec![first_id, second_id];
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM twitch_crew_review_events WHERE id = ANY($1::bigint[]) FOR UPDATE")
        .bind(&event_ids)
        .fetch_all(&mut *blocker)
        .await
        .unwrap();
    let marking_store = store.clone();
    let marking_ids = event_ids.clone();
    let marking = tokio::spawn(async move {
        marking_store
            .mark_discord_cards_sent(
                &[
                    DiscordCard {
                        event_ids: vec![marking_ids[0]],
                        message_id: "discord-lock-first".to_owned(),
                    },
                    DiscordCard {
                        event_ids: vec![marking_ids[1]],
                        message_id: "discord-lock-second".to_owned(),
                    },
                ],
                cycle.claim_id,
            )
            .await
    });
    wait_for_lock_query(&pool, "SELECT id, review_session_id").await;
    let active_during_wait: bool = sqlx::query_scalar(
        "UPDATE twitch_crew_review_events
            SET discord_claim_until = clock_timestamp() + INTERVAL '200 milliseconds'
          WHERE discord_claim_id = $1
        RETURNING discord_claim_until > clock_timestamp()",
    )
    .bind(cycle.claim_id)
    .fetch_one(&mut *blocker)
    .await
    .unwrap();
    assert!(active_during_wait);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let expired_during_wait: bool = sqlx::query_scalar(
        "SELECT BOOL_AND(discord_claim_until <= clock_timestamp())
           FROM twitch_crew_review_events
          WHERE discord_claim_id = $1",
    )
    .bind(cycle.claim_id)
    .fetch_one(&mut *blocker)
    .await
    .unwrap();
    assert!(expired_during_wait);
    blocker.commit().await.unwrap();

    let result = marking.await.unwrap();
    assert!(matches!(result, Err(StoreError::InvalidClaim)));
    let rows: Vec<(Option<String>, Option<Uuid>)> = sqlx::query_as(
        "SELECT discord_message_id, discord_claim_id
           FROM twitch_crew_review_events
          WHERE id = ANY($1::bigint[])
          ORDER BY id",
    )
    .bind(&event_ids)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![(None, Some(cycle.claim_id)), (None, Some(cycle.claim_id))]
    );
    let retried = store.pending_discord_cycles(1).await.unwrap().remove(0);
    assert_ne!(retried.claim_id, cycle.claim_id);
}

#[tokio::test]
async fn ueberlanges_terminal_rollt_draft_zurueck_und_behaelt_claim() {
    let Some(pool) = test_pool("crew_review_oversized_terminal_completion").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let channel = "oversized-terminal-completion";
    let session_id = seed_session(&pool, channel, now).await;
    let cycle_id = Uuid::new_v4();
    store
        .append_event(event(
            session_id,
            channel,
            ReviewEventKind::RickyMessage,
            now,
            cycle_id,
        ))
        .await
        .unwrap();
    let claim = store
        .pending_model_inputs(session_id)
        .await
        .unwrap()
        .unwrap();
    let mut terminal = event(
        session_id,
        channel,
        ReviewEventKind::AiDecision,
        now + chrono::Duration::seconds(2),
        cycle_id,
    );
    terminal.content = Some("x".repeat(1_201));

    let result = store
        .complete_claimed_model_cycle(
            claim.claim_id,
            Some(event(
                session_id,
                channel,
                ReviewEventKind::AiDraft,
                now + chrono::Duration::seconds(1),
                cycle_id,
            )),
            terminal,
        )
        .await;
    assert!(matches!(result, Err(StoreError::InvalidClaim)));
    let generated: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
           FROM twitch_crew_review_events
          WHERE review_session_id = $1
            AND metadata->>'cycle_id' = $2
            AND event_kind IN ('ai_draft', 'ai_decision', 'provider_error')",
    )
    .bind(session_id)
    .bind(cycle_id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(generated, 0);
    let retained_claim: Option<Uuid> = sqlx::query_scalar(
        "SELECT model_claim_id
           FROM twitch_crew_review_events
          WHERE review_session_id = $1
            AND metadata->>'cycle_id' = $2
            AND event_kind = 'ricky_message'",
    )
    .bind(session_id)
    .bind(cycle_id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(retained_claim, Some(claim.claim_id));
}

#[tokio::test]
async fn ueberlanges_terminal_wird_auch_beim_einzelappend_fail_closed_abgelehnt() {
    let Some(pool) = test_pool("crew_review_oversized_terminal_append").await else {
        return;
    };
    let store = CrewReviewStore::new(pool.clone());
    let now = Utc::now();
    let channel = "oversized-terminal-append";
    let session_id = seed_session(&pool, channel, now).await;
    let cycle_id = Uuid::new_v4();
    store
        .append_event(event(
            session_id,
            channel,
            ReviewEventKind::StreamerTranscript,
            now,
            cycle_id,
        ))
        .await
        .unwrap();
    let claim = store
        .pending_model_inputs(session_id)
        .await
        .unwrap()
        .unwrap();
    let mut terminal = event(
        session_id,
        channel,
        ReviewEventKind::ProviderError,
        now + chrono::Duration::seconds(1),
        cycle_id,
    );
    terminal.content = Some("x".repeat(1_201));

    let result = store
        .append_claimed_model_event(claim.claim_id, terminal)
        .await;
    assert!(matches!(result, Err(StoreError::InvalidClaim)));
    let terminal_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
           FROM twitch_crew_review_events
          WHERE review_session_id = $1
            AND metadata->>'cycle_id' = $2
            AND event_kind IN ('ai_decision', 'provider_error')",
    )
    .bind(session_id)
    .bind(cycle_id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(terminal_count, 0);
    let retained_claim: Option<Uuid> = sqlx::query_scalar(
        "SELECT model_claim_id
           FROM twitch_crew_review_events
          WHERE review_session_id = $1
            AND metadata->>'cycle_id' = $2
            AND event_kind = 'streamer_transcript'",
    )
    .bind(session_id)
    .bind(cycle_id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(retained_claim, Some(claim.claim_id));
}
