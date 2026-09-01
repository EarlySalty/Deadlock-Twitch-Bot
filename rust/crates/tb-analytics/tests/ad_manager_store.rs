use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::str::FromStr;
use tb_analytics::ad_manager::{AdManagerStore, EnqueueOutcome, Settings};
use tb_transport_twitch::AdSchedule;

const MIGRATION: &str = include_str!("../../../migrations/20260901100000_twitch_ad_manager.sql");

#[test]
fn schema_enthaelt_queue_und_current_state_invarianten() {
    for required in [
        "CREATE TABLE IF NOT EXISTS twitch_ad_manager_settings",
        "CREATE TABLE IF NOT EXISTS twitch_ad_manager_state",
        "CREATE TABLE IF NOT EXISTS twitch_ad_manager_actions",
        "idempotency_key TEXT NOT NULL UNIQUE",
        "due_at TIMESTAMPTZ NOT NULL",
        "lease_until TIMESTAMPTZ",
        "attempt_count INTEGER NOT NULL",
        "preflight_next_ad_at TIMESTAMPTZ",
        "worker_heartbeat_at TIMESTAMPTZ",
        "observed_at TIMESTAMPTZ,",
        "'unknown', 'unresolved', 'cancelled'",
        "WHERE status IN ('pending', 'leased', 'unknown')",
        "twitch_ad_manager_actions_completed_retention",
    ] {
        assert!(MIGRATION.contains(required), "Schema fehlt: {required}");
    }
}

#[tokio::test]
async fn queue_lease_idempotenz_und_state_sind_atomar() {
    let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
        return;
    };
    let schema = format!("t_ad_manager_{}", std::process::id());
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
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
    let options = PgConnectOptions::from_str(&dsn)
        .unwrap()
        .options([("search_path", schema.as_str())]);
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::raw_sql(MIGRATION).execute(&pool).await.unwrap();
    let store = AdManagerStore::new(pool.clone());

    store
        .save_settings("42", "nani", &Settings::default())
        .await
        .unwrap();
    let managed = store.list_channels().await.unwrap();
    assert_eq!(managed.len(), 1);
    assert_eq!(managed[0].twitch_user_id, "42");
    assert!(!managed[0].settings.enabled);
    let state_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM twitch_ad_manager_state WHERE twitch_user_id='42')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        !state_exists,
        "Speichern darf keinen Messzeitpunkt erfinden"
    );
    store.touch_worker("42", "nani").await.unwrap();
    let (observed_at, heartbeat_at): (
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT observed_at,worker_heartbeat_at FROM twitch_ad_manager_state WHERE twitch_user_id='42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        observed_at.is_none(),
        "Heartbeat ist keine Twitch-Beobachtung"
    );
    assert!(heartbeat_at.is_some());
    let worker_lease = store.try_acquire_worker_lease("42").await.unwrap().unwrap();
    assert!(store
        .try_acquire_worker_lease("42")
        .await
        .unwrap()
        .is_none());
    assert!(!store.release_worker_lease("42", "fremd").await.unwrap());
    assert!(store
        .release_worker_lease("42", &worker_lease)
        .await
        .unwrap());

    assert_eq!(
        store
            .enqueue("42", "nani", "snooze", None, "42", "manual-1")
            .await
            .unwrap(),
        EnqueueOutcome::Queued
    );
    assert_eq!(
        store
            .enqueue("42", "nani", "snooze", None, "42", "manual-2")
            .await
            .unwrap(),
        EnqueueOutcome::Conflict,
    );
    assert_eq!(
        store
            .enqueue("42", "nani", "snooze", None, "42", "manual-1")
            .await
            .unwrap(),
        EnqueueOutcome::AlreadyAccepted,
    );

    let first = store.claim_due("42").await.unwrap().unwrap();
    assert_eq!(first.source, "manual");
    assert!(store.claim_due("42").await.unwrap().is_none());
    sqlx::query(
        "UPDATE twitch_ad_manager_actions SET lease_until=NOW()-INTERVAL '1 second' WHERE id=$1",
    )
    .bind(first.id)
    .execute(&pool)
    .await
    .unwrap();
    let second = store.claim_due("42").await.unwrap().unwrap();
    assert_ne!(first.lease_token, second.lease_token);
    assert!(
        store
            .finish_action(&first, "succeeded", None, None)
            .await
            .is_err(),
        "ein alter Lease darf weder Action noch State finalisieren"
    );
    let preflight = AdSchedule {
        next_ad_at: Some("2026-09-01T15:00:00Z".into()),
        last_ad_at: Some("2026-09-01T14:00:00Z".into()),
        snooze_count: 2,
        ..AdSchedule::default()
    };
    store
        .mark_unknown_before_send(&second, &preflight, chrono::Utc::now())
        .await
        .unwrap();
    let unknown_actions = store.unknown_actions("42").await.unwrap();
    assert_eq!(unknown_actions.len(), 1);
    assert_eq!(unknown_actions[0].preflight_snooze_count, Some(2));
    let unknown: Option<String> = sqlx::query_scalar(
        "SELECT last_action_outcome FROM twitch_ad_manager_state WHERE twitch_user_id='42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(unknown.as_deref(), Some("unknown"));
    assert_eq!(
        store
            .enqueue("42", "nani", "snooze", None, "42", "manual-neu")
            .await
            .unwrap(),
        EnqueueOutcome::Conflict,
    );
    let expiry_now = chrono::Utc::now();
    sqlx::query("UPDATE twitch_ad_manager_actions SET marked_unknown_at=$2-INTERVAL '15 minutes' WHERE id=$1")
        .bind(second.id)
        .bind(expiry_now)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        store
            .expire_unknown_actions("42", expiry_now - chrono::Duration::milliseconds(1))
            .await
            .unwrap(),
        0,
        "vor der Schutzfrist bleibt die Aktion gesperrt"
    );
    assert_eq!(
        store
            .expire_unknown_actions("42", expiry_now)
            .await
            .unwrap(),
        1
    );
    let unresolved: String =
        sqlx::query_scalar("SELECT status FROM twitch_ad_manager_actions WHERE id=$1")
            .bind(second.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(unresolved, "unresolved");
    assert_eq!(
        store
            .enqueue("42", "nani", "snooze", None, "42", "manual-neu")
            .await
            .unwrap(),
        EnqueueOutcome::Queued,
        "Expiry muss die Unique-Sperre ohne Live- oder Schedule-Beleg lösen"
    );
    let second = store.claim_due("42").await.unwrap().unwrap();
    store
        .finish_action(&second, "succeeded", Some("ok"), None)
        .await
        .unwrap();

    let outcome: Option<String> = sqlx::query_scalar(
        "SELECT last_action_outcome FROM twitch_ad_manager_state WHERE twitch_user_id='42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(outcome.as_deref(), Some("succeeded"));
    assert_eq!(
        store
            .enqueue("42", "nani", "snooze", None, "42", "manual-1")
            .await
            .unwrap(),
        EnqueueOutcome::AlreadyAccepted,
    );
    assert!(store
        .enqueue_automatic("42", "nani", "commercial", Some(90), "auto-cancel")
        .await
        .unwrap());
    store
        .save_settings("42", "nani", &Settings::default())
        .await
        .unwrap();
    let cancelled: String = sqlx::query_scalar(
        "SELECT status FROM twitch_ad_manager_actions WHERE idempotency_key='auto-cancel'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cancelled, "cancelled");

    sqlx::query("INSERT INTO twitch_ad_manager_actions(twitch_user_id,twitch_login,action,duration_seconds,source,status,idempotency_key,requested_by_twitch_user_id,completed_at,created_at) SELECT '42','nani','commercial',30,'manual','succeeded','rate-'||n,'42',NOW(),NOW() FROM generate_series(1,11) n")
        .execute(&pool).await.unwrap();
    assert_eq!(
        store
            .enqueue("42", "nani", "commercial", Some(30), "42", "rate-limit")
            .await
            .unwrap(),
        EnqueueOutcome::RateLimited
    );

    sqlx::query("INSERT INTO twitch_ad_manager_actions(twitch_user_id,twitch_login,action,duration_seconds,source,status,idempotency_key,requested_by_twitch_user_id,completed_at,created_at) VALUES('42','nani','commercial',30,'manual','failed','retention-alt','42',NOW()-INTERVAL '91 days',NOW()-INTERVAL '91 days'),('42','nani','commercial',30,'manual','unresolved','retention-unresolved','42',NOW()-INTERVAL '91 days',NOW()-INTERVAL '91 days'),('42','nani','commercial',30,'manual','failed','retention-neu','42',NOW()-INTERVAL '89 days',NOW()-INTERVAL '89 days')")
        .execute(&pool).await.unwrap();
    assert_eq!(store.cleanup_completed_actions().await.unwrap(), 2);
    let recent_exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM twitch_ad_manager_actions WHERE idempotency_key='retention-neu')").fetch_one(&pool).await.unwrap();
    assert!(recent_exists);

    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
