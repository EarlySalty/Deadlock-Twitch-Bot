//! Hermetische tb-monitoring-Tests gegen den Wegwerf-Container
//! (`TB_TEST_DATABASE_URL`, siehe `rust/scripts/test_db.sh up`).
//!
//! Jeder Test bekommt sein eigenes Postgres-Schema (via `search_path`),
//! weil die Tests parallel laufen und `lease_due` sonst fremde Aufträge
//! stehlen würde. Das DDL bildet exakt das von Python angelegte
//! Prod-Schema nach (Epoch-Sekunden als DOUBLE PRECISION).

use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_monitoring::{
    ClockFn, DeadLetterHook, DeadLetterNotice, GuardKind, GuardStore, HandlerError,
    InboxHandler, InboxRuntime, ProcessingInboxStore,
};

fn test_dsn() -> Option<String> {
    std::env::var("TB_TEST_DATABASE_URL").ok()
}

macro_rules! skip_without_db {
    () => {
        match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!(
                    "SKIP: TB_TEST_DATABASE_URL nicht gesetzt — `rust/scripts/test_db.sh up`"
                );
                return;
            }
        }
    };
}

/// Frisches Schema + Tabellen (Prod-DDL) + Pool mit `search_path` aufs Schema.
async fn pool_in_schema(dsn: &str, schema: &str) -> PgPool {
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(dsn)
        .await
        .expect("admin connect");
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
        .expect("dsn parse")
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .expect("connect");

    for ddl in [
        "CREATE TABLE eventsub_guard_state (
            kind TEXT NOT NULL,
            guard_key TEXT NOT NULL,
            expires_at DOUBLE PRECISION NOT NULL,
            updated_at DOUBLE PRECISION NOT NULL,
            PRIMARY KEY (kind, guard_key)
        )",
        "CREATE TABLE twitch_eventsub_processing_inbox (
            work_id          TEXT PRIMARY KEY,
            work_type        TEXT NOT NULL,
            message_id       TEXT,
            payload_json     TEXT NOT NULL,
            queued_at        DOUBLE PRECISION NOT NULL,
            next_attempt_at  DOUBLE PRECISION NOT NULL,
            attempt_count    INTEGER NOT NULL DEFAULT 0,
            last_error       TEXT
        )",
        "CREATE TABLE twitch_eventsub_processing_dead_letter (
            work_id           TEXT PRIMARY KEY,
            work_type         TEXT NOT NULL,
            message_id        TEXT,
            payload_json      TEXT NOT NULL,
            queued_at         DOUBLE PRECISION NOT NULL,
            dead_lettered_at  DOUBLE PRECISION NOT NULL,
            attempt_count     INTEGER NOT NULL,
            last_error        TEXT
        )",
    ] {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }
    pool
}

// ── Guard-Store ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn guard_claim_gewinnt_nur_einmal_bis_ttl_ablauf() {
    let dsn = skip_without_db!();
    let pool = pool_in_schema(&dsn, "t4a_guard_claim").await;
    let guard = GuardStore::new(pool);

    // Erster Claim gewinnt, zweiter innerhalb des TTL verliert.
    assert!(guard.claim(GuardKind::MessageId, "msg-1", 10.0, 100.0).await.unwrap());
    assert!(!guard.claim(GuardKind::MessageId, "msg-1", 10.0, 105.0).await.unwrap());
    assert!(guard.is_active(GuardKind::MessageId, "msg-1", 105.0).await.unwrap());

    // expires_at = 110: ab now >= 110 ist der Guard frei und claimbar.
    assert!(!guard.is_active(GuardKind::MessageId, "msg-1", 110.0).await.unwrap());
    assert!(guard.claim(GuardKind::MessageId, "msg-1", 10.0, 110.0).await.unwrap());

    // Gleicher Key unter anderem Kind kollidiert nicht.
    assert!(guard.claim(GuardKind::OfflineThrottle, "msg-1", 10.0, 100.0).await.unwrap());
}

#[tokio::test]
async fn guard_release_und_leerer_key() {
    let dsn = skip_without_db!();
    let pool = pool_in_schema(&dsn, "t4a_guard_release").await;
    let guard = GuardStore::new(pool);

    assert!(guard.claim(GuardKind::BusinessEffect, "fx-1", 604800.0, 50.0).await.unwrap());
    assert!(!guard.claim(GuardKind::BusinessEffect, "fx-1", 604800.0, 51.0).await.unwrap());
    guard.release(GuardKind::BusinessEffect, "fx-1").await.unwrap();
    assert!(guard.claim(GuardKind::BusinessEffect, "fx-1", 604800.0, 52.0).await.unwrap());

    // Leere/Whitespace-Keys: kein Claim, nicht aktiv (wie Python).
    assert!(!guard.claim(GuardKind::BusinessEffect, "   ", 10.0, 0.0).await.unwrap());
    assert!(!guard.is_active(GuardKind::BusinessEffect, "", 0.0).await.unwrap());
}

#[tokio::test]
async fn guard_sweep_loescht_nur_abgelaufene() {
    let dsn = skip_without_db!();
    let pool = pool_in_schema(&dsn, "t4a_guard_sweep").await;
    let guard = GuardStore::new(pool);

    assert!(guard.claim(GuardKind::MessageId, "kurz", 10.0, 100.0).await.unwrap()); // expires 110
    assert!(guard.claim(GuardKind::MessageId, "lang", 100.0, 100.0).await.unwrap()); // expires 200

    assert_eq!(guard.sweep_expired(150.0).await.unwrap(), 1);
    assert!(!guard.is_active(GuardKind::MessageId, "kurz", 150.0).await.unwrap());
    assert!(guard.is_active(GuardKind::MessageId, "lang", 150.0).await.unwrap());
}

// ── Inbox-Store ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn inbox_enqueue_lease_deliver() {
    let dsn = skip_without_db!();
    let pool = pool_in_schema(&dsn, "t4a_inbox_lease").await;
    let store = ProcessingInboxStore::new(pool);
    let payload = serde_json::json!({"subscription": {"type": "stream.online"}});

    let id = store.enqueue("stream.online", &payload, Some("m-1"), 1000.0).await.unwrap();
    assert_eq!(id.len(), 32, "uuid4().hex-Format ohne Bindestriche");

    // Sofort fällig; Lease schiebt next_attempt_at um 30 s.
    let leased = store.lease_due(1000.0, 30.0, 20).await.unwrap();
    assert_eq!(leased.len(), 1);
    assert_eq!(leased[0].work_id, id);
    assert_eq!(leased[0].work_type, "stream.online");
    assert_eq!(leased[0].message_id.as_deref(), Some("m-1"));
    assert_eq!(leased[0].attempt_count, 0);

    // Während des Lease zieht ein zweiter Worker nichts …
    assert!(store.lease_due(1001.0, 30.0, 20).await.unwrap().is_empty());
    // … nach Lease-Ablauf ist der Auftrag wieder fällig (Crash-Recovery).
    assert_eq!(store.lease_due(1031.0, 30.0, 20).await.unwrap().len(), 1);

    store.mark_delivered(&id).await.unwrap();
    assert!(store.list_pending(10).await.unwrap().is_empty());
}

#[tokio::test]
async fn inbox_retry_dead_letter_requeue() {
    let dsn = skip_without_db!();
    let pool = pool_in_schema(&dsn, "t4a_inbox_retry").await;
    let store = ProcessingInboxStore::new(pool);
    let payload = serde_json::json!({"event": {"broadcaster_user_id": "42"}});

    let id = store.enqueue("channel.raid", &payload, None, 1000.0).await.unwrap();
    let leased = store.lease_due(1000.0, 30.0, 20).await.unwrap();
    assert_eq!(leased.len(), 1);

    // Retry: Zähler, Fälligkeit, Fehlertext.
    store.mark_retry(&id, 1, "boom", 2000.0).await.unwrap();
    let pending = store.list_pending(10).await.unwrap();
    assert_eq!(pending[0].attempt_count, 1);
    assert_eq!(pending[0].last_error.as_deref(), Some("boom"));
    assert_eq!(pending[0].next_attempt_at, 2000.0);
    assert!(store.lease_due(1999.0, 30.0, 20).await.unwrap().is_empty());

    let leased = store.lease_due(2000.0, 30.0, 20).await.unwrap();
    assert_eq!(leased[0].attempt_count, 1);

    // Dead-Letter: atomar verschoben, Fehlertext auf 500 Zeichen gekürzt.
    let long_error = "x".repeat(800);
    store.mark_dead_letter(&leased[0], 5, &long_error, 2100.0).await.unwrap();
    assert!(store.list_pending(10).await.unwrap().is_empty());
    let dead = store.list_dead_letters(10).await.unwrap();
    assert_eq!(dead.len(), 1);
    assert_eq!(dead[0].attempt_count, 5);
    assert_eq!(dead[0].last_error.as_ref().unwrap().chars().count(), 500);
    assert_eq!(dead[0].dead_lettered_at, 2100.0);

    // Requeue: zurück in die Inbox mit Zähler 0, sofort fällig.
    assert!(store.requeue_dead_letter(&id, 2200.0).await.unwrap());
    assert!(store.list_dead_letters(10).await.unwrap().is_empty());
    let pending = store.list_pending(10).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].attempt_count, 0);
    assert_eq!(pending[0].next_attempt_at, 2200.0);

    // Unbekannte/leere work_id → false.
    assert!(!store.requeue_dead_letter(&id, 2300.0).await.unwrap());
    assert!(!store.requeue_dead_letter("  ", 2300.0).await.unwrap());
}

// ── Inbox-Runtime ─────────────────────────────────────────────────────────────

struct CountingHandler {
    calls: AtomicU64,
}

#[async_trait::async_trait]
impl InboxHandler for CountingHandler {
    async fn handle(&self, _work_type: &str, _payload: &serde_json::Value) -> Result<(), HandlerError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct FailingHandler;

#[async_trait::async_trait]
impl InboxHandler for FailingHandler {
    async fn handle(&self, _work_type: &str, _payload: &serde_json::Value) -> Result<(), HandlerError> {
        Err("kaputt".into())
    }
}

struct RecordingHook {
    notices: tokio::sync::Mutex<Vec<DeadLetterNotice>>,
}

#[async_trait::async_trait]
impl DeadLetterHook for RecordingHook {
    async fn on_dead_letter(&self, notice: DeadLetterNotice) {
        self.notices.lock().await.push(notice);
    }
}

/// Wartet bis `cond` true liefert (max. ~5 s) — Worker arbeitet asynchron.
async fn wait_until<F, Fut>(mut cond: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..100 {
        if cond().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test]
async fn runtime_verarbeitet_auftrag_und_loescht_ihn() {
    let dsn = skip_without_db!();
    let pool = pool_in_schema(&dsn, "t4a_rt_ok").await;
    let store = ProcessingInboxStore::new(pool);
    let handler = Arc::new(CountingHandler { calls: AtomicU64::new(0) });

    let runtime = InboxRuntime::new(store.clone(), handler.clone()).start();
    runtime
        .enqueue("stream.online", &serde_json::json!({"x": 1}), Some("m-7"))
        .await
        .unwrap();

    let done = wait_until(|| {
        let store = store.clone();
        async move { store.list_pending(5).await.unwrap().is_empty() }
    })
    .await;
    runtime.shutdown().await;

    assert!(done, "Auftrag wurde nicht verarbeitet");
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    assert!(store.list_dead_letters(5).await.unwrap().is_empty());
}

#[tokio::test]
async fn runtime_dead_lettert_nach_max_versuchen() {
    let dsn = skip_without_db!();
    let pool = pool_in_schema(&dsn, "t4a_rt_dead").await;
    let store = ProcessingInboxStore::new(pool);
    let hook = Arc::new(RecordingHook { notices: tokio::sync::Mutex::new(Vec::new()) });

    // Uhr springt pro Ablesung 61 s vorwärts — jeder Retry (Backoff-Cap 60 s)
    // ist damit beim nächsten Poll sofort fällig; der Test braucht keine Echtzeit.
    let ticks = Arc::new(AtomicU64::new(1_000_000));
    let clock: ClockFn = {
        let ticks = ticks.clone();
        Arc::new(move || ticks.fetch_add(61, Ordering::SeqCst) as f64)
    };

    store
        .enqueue("channel.update", &serde_json::json!({"y": 2}), None, 1_000_000.0)
        .await
        .unwrap();

    let runtime = InboxRuntime::new(store.clone(), Arc::new(FailingHandler))
        .with_dead_letter_hook(hook.clone())
        .with_clock(clock)
        .start();

    let done = wait_until(|| {
        let store = store.clone();
        async move { !store.list_dead_letters(5).await.unwrap().is_empty() }
    })
    .await;
    runtime.shutdown().await;

    assert!(done, "Auftrag wurde nicht dead-lettered");
    assert!(store.list_pending(5).await.unwrap().is_empty());
    let dead = store.list_dead_letters(5).await.unwrap();
    assert_eq!(dead[0].attempt_count, 5);
    assert_eq!(dead[0].last_error.as_deref(), Some("kaputt"));

    let notices = hook.notices.lock().await;
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].attempt_count, 5);
    assert_eq!(notices[0].payload, serde_json::json!({"y": 2}));
}

#[tokio::test]
async fn runtime_dead_lettert_kaputtes_payload_json() {
    let dsn = skip_without_db!();
    let pool = pool_in_schema(&dsn, "t4a_rt_badjson").await;
    let store = ProcessingInboxStore::new(pool.clone());

    // Kaputtes JSON direkt in die Tabelle — über enqueue ist das nicht erzeugbar.
    sqlx::query(
        "INSERT INTO twitch_eventsub_processing_inbox
            (work_id, work_type, message_id, payload_json, queued_at, next_attempt_at, attempt_count)
         VALUES ('badjson01', 'stream.online', NULL, '{nicht-json', 1000000, 1000000, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let ticks = Arc::new(AtomicU64::new(1_000_000));
    let clock: ClockFn = {
        let ticks = ticks.clone();
        Arc::new(move || ticks.fetch_add(61, Ordering::SeqCst) as f64)
    };
    // Handler darf nie aufgerufen werden — kaputtes JSON scheitert davor.
    let handler = Arc::new(CountingHandler { calls: AtomicU64::new(0) });
    let runtime = InboxRuntime::new(store.clone(), handler.clone())
        .with_clock(clock)
        .start();

    let done = wait_until(|| {
        let store = store.clone();
        async move { !store.list_dead_letters(5).await.unwrap().is_empty() }
    })
    .await;
    runtime.shutdown().await;

    assert!(done, "kaputtes Payload wurde nicht dead-lettered");
    assert_eq!(handler.calls.load(Ordering::SeqCst), 0);
    let dead = store.list_dead_letters(5).await.unwrap();
    assert!(dead[0].last_error.as_deref().unwrap().contains("invalid eventsub processing payload"));
}
