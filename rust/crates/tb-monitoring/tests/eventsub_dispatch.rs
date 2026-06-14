//! Hermetische Tests des EventSub-Ingress (Slice 4d): Dispatch-Dedup,
//! Inbox-Verarbeitung der Core-Typen, Telemetrie-Inserts, Offline-Throttle.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{Duration, Utc};
use sqlx::PgPool;
use tb_monitoring::poller::source::{ChannelInfo, ChannelInfoSource, SourceError};
use tb_monitoring::sessions::store::SessionStore;
use tb_monitoring::{
    epoch_clock, EventSubDispatcher, EventSubHooks, ExpSessionStore, ExpSessionTracker, GuardStore,
    HypeTrainPhase, InboxRuntime, InboxRuntimeHandle, LiveStateStore, MonitoringEventHandler,
    NoFollowerSource, ProcessingInboxStore, SessionTracker, StreamSnapshot, TelemetryStore,
};

mod support;

macro_rules! pool_or_skip {
    ($schema:expr) => {
        match support::pool_in_schema($schema).await {
            Some(pool) => pool,
            None => return,
        }
    };
}

/// Zählt Hook-Aufrufe.
#[derive(Default)]
struct RecordingHooks {
    went_live: AtomicU64,
    score_refresh: AtomicU64,
    stream_offline: AtomicU64,
    channel_raid: AtomicU64,
}

#[async_trait::async_trait]
impl EventSubHooks for RecordingHooks {
    async fn on_channel_raid(&self, _event: &serde_json::Value, _message_id: Option<&str>) {
        self.channel_raid.fetch_add(1, Ordering::SeqCst);
    }
    async fn on_stream_went_live(&self, _twitch_user_id: &str, _login: &str) {
        self.went_live.fetch_add(1, Ordering::SeqCst);
    }
    async fn on_score_refresh(
        &self,
        _twitch_user_id: &str,
        _login: Option<&str>,
        _trigger: &'static str,
    ) {
        self.score_refresh.fetch_add(1, Ordering::SeqCst);
    }
    async fn on_stream_offline(&self, _twitch_user_id: &str, _login: Option<&str>) {
        self.stream_offline.fetch_add(1, Ordering::SeqCst);
    }
}

/// Statische Kanal-Metadaten für das Go-Live-Enrichment im Test.
struct StaticChannelInfo;

#[async_trait::async_trait]
impl ChannelInfoSource for StaticChannelInfo {
    async fn channel_info(
        &self,
        _broadcaster_id: &str,
    ) -> Result<Option<ChannelInfo>, SourceError> {
        Ok(Some(ChannelInfo {
            title: Some("Ranked Grind".to_string()),
            game_name: Some("Deadlock".to_string()),
        }))
    }
}

fn build_stack(
    pool: &PgPool,
    hooks: Arc<RecordingHooks>,
) -> (EventSubDispatcher, InboxRuntimeHandle, ProcessingInboxStore) {
    build_stack_with(pool, hooks, None)
}

fn build_stack_with(
    pool: &PgPool,
    hooks: Arc<RecordingHooks>,
    channel_info: Option<Arc<dyn ChannelInfoSource>>,
) -> (EventSubDispatcher, InboxRuntimeHandle, ProcessingInboxStore) {
    let guard = GuardStore::new(pool.clone());
    let telemetry = TelemetryStore::new(pool.clone());
    let live_state = LiveStateStore::new(pool.clone());
    let tracker = Arc::new(SessionTracker::new(
        SessionStore::new(pool.clone()),
        live_state.clone(),
        ExpSessionTracker::new(ExpSessionStore::new(pool.clone())),
        Arc::new(NoFollowerSource),
        "Deadlock",
    ));
    let handler = Arc::new(MonitoringEventHandler::new(
        guard.clone(),
        live_state,
        tracker,
        telemetry.clone(),
        hooks.clone(),
        channel_info,
        Arc::new(epoch_clock),
    ));
    let store = ProcessingInboxStore::new(pool.clone());
    let runtime = InboxRuntime::new(store.clone(), handler).start();
    let dispatcher = EventSubDispatcher::new(
        guard,
        runtime.enqueuer(),
        telemetry,
        hooks,
        Arc::new(epoch_clock),
    );
    (dispatcher, runtime, store)
}

async fn wait_until_empty(store: &ProcessingInboxStore) -> bool {
    for _ in 0..100 {
        if store.list_pending(5).await.unwrap().is_empty() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test]
async fn stream_online_dispatch_dedup_und_verarbeitung() {
    let pool = pool_or_skip!("t4d_online");
    let hooks = Arc::new(RecordingHooks::default());
    let (dispatcher, runtime, store) = build_stack(&pool, hooks.clone());

    let body = serde_json::json!({
        "subscription": {"type": "stream.online"},
        "event": {
            "broadcaster_user_id": "42",
            "broadcaster_user_login": "drag",
            "id": "s-77",
            "started_at": "2026-06-09T17:00:00Z"
        }
    });
    let outcome = dispatcher
        .dispatch("stream.online", Some("m-1"), &body)
        .await
        .unwrap();
    assert!(outcome.ok && outcome.queued && !outcome.duplicate);

    // Gleiche Message erneut → Duplikat, nichts Neues in der Inbox.
    let dup = dispatcher
        .dispatch("stream.online", Some("m-1"), &body)
        .await
        .unwrap();
    assert!(dup.duplicate && !dup.queued);

    assert!(wait_until_empty(&store).await, "Inbox nicht abgearbeitet");
    runtime.shutdown().await;

    #[derive(sqlx::FromRow)]
    struct StateRow {
        streamer_login: String,
        is_live: Option<i32>,
        last_stream_id: Option<String>,
        last_started_at: Option<String>,
    }
    let state: StateRow = sqlx::query_as(
        "SELECT streamer_login, is_live, last_stream_id, last_started_at
           FROM twitch_live_state WHERE twitch_user_id = '42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state.streamer_login, "drag");
    assert_eq!(state.is_live, Some(1));
    assert_eq!(state.last_stream_id.as_deref(), Some("s-77"));
    assert_eq!(
        state.last_started_at.as_deref(),
        Some("2026-06-09T17:00:00Z"),
        "Roh-Wert aus dem Event (Python übernimmt ihn unverändert)"
    );
    assert_eq!(hooks.went_live.load(Ordering::SeqCst), 1);
    assert_eq!(hooks.score_refresh.load(Ordering::SeqCst), 1);
}

/// Go-Live-Enrichment: stream.online holt Titel/Kategorie über den
/// ChannelInfoSource-Port und schreibt sie sofort in den Live-State —
/// last_game ist damit nicht mehr poll-abhängig (Auto-Raid-Grundlage).
#[tokio::test]
async fn stream_online_enrichment_setzt_titel_und_kategorie() {
    let pool = pool_or_skip!("t4d_online_enrich");
    let hooks = Arc::new(RecordingHooks::default());
    let (dispatcher, runtime, store) = build_stack_with(
        &pool,
        hooks.clone(),
        Some(Arc::new(StaticChannelInfo) as Arc<dyn ChannelInfoSource>),
    );

    let body = serde_json::json!({
        "subscription": {"type": "stream.online"},
        "event": {
            "broadcaster_user_id": "99",
            "broadcaster_user_login": "trippy",
            "id": "s-99",
            "started_at": "2026-06-10T12:00:00Z"
        }
    });
    let outcome = dispatcher
        .dispatch("stream.online", Some("m-enrich-1"), &body)
        .await
        .unwrap();
    assert!(outcome.ok && outcome.queued);
    assert!(wait_until_empty(&store).await, "Inbox nicht abgearbeitet");
    runtime.shutdown().await;

    #[derive(sqlx::FromRow)]
    struct StateRow {
        last_title: Option<String>,
        last_game: Option<String>,
    }
    let state: StateRow = sqlx::query_as(
        "SELECT last_title, last_game FROM twitch_live_state WHERE twitch_user_id = '99'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state.last_title.as_deref(), Some("Ranked Grind"));
    assert_eq!(state.last_game.as_deref(), Some("Deadlock"));
}

#[tokio::test]
async fn stream_offline_finalisiert_session_mit_throttle() {
    let pool = pool_or_skip!("t4d_offline");
    let hooks = Arc::new(RecordingHooks::default());
    let (dispatcher, runtime, store) = build_stack(&pool, hooks.clone());

    // Offene Session + Live-State herstellen.
    sqlx::query(
        "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live)
         VALUES ('42', 'drag', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let tracker = SessionTracker::new(
        SessionStore::new(pool.clone()),
        LiveStateStore::new(pool.clone()),
        ExpSessionTracker::new(ExpSessionStore::new(pool.clone())),
        Arc::new(NoFollowerSource),
        "Deadlock",
    );
    let stream = StreamSnapshot {
        id: Some("s-1".to_string()),
        user_login: "drag".to_string(),
        user_id: "0".to_string(),
        game_name: "Deadlock".to_string(),
        viewer_count: 5,
        started_at: Some((Utc::now() - Duration::minutes(30)).to_rfc3339()),
        ..Default::default()
    };
    tracker
        .ensure_session("drag", &stream, None, Some("42"), Utc::now())
        .await
        .expect("session");

    let body = serde_json::json!({
        "subscription": {"type": "stream.offline"},
        "event": {"broadcaster_user_id": "42", "broadcaster_user_login": "drag"}
    });
    let outcome = dispatcher
        .dispatch("stream.offline", Some("m-off-1"), &body)
        .await
        .unwrap();
    assert!(outcome.queued);
    assert!(wait_until_empty(&store).await);

    let (is_live, active): (Option<i32>, Option<i64>) = sqlx::query_as(
        "SELECT is_live, active_session_id FROM twitch_live_state WHERE twitch_user_id = '42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(is_live, Some(0));
    assert_eq!(active, None);
    let (ended, notes): (Option<chrono::DateTime<Utc>>, Option<String>) =
        sqlx::query_as("SELECT ended_at, notes FROM twitch_stream_sessions LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(ended.is_some());
    assert_eq!(notes.as_deref(), Some("offline"));
    assert_eq!(hooks.stream_offline.load(Ordering::SeqCst), 1);

    // Zweites Offline-Event (andere Message) im 120s-Fenster → gedrosselt.
    let outcome = dispatcher
        .dispatch("stream.offline", Some("m-off-2"), &body)
        .await
        .unwrap();
    assert!(outcome.queued);
    assert!(wait_until_empty(&store).await);
    runtime.shutdown().await;
    assert_eq!(
        hooks.stream_offline.load(Ordering::SeqCst),
        1,
        "Offline-Throttle verhindert Doppel-Trigger"
    );
}

#[tokio::test]
async fn telemetrie_und_channel_update_und_raid_hook() {
    let pool = pool_or_skip!("t4d_telemetry");
    let hooks = Arc::new(RecordingHooks::default());
    let (dispatcher, runtime, store) = build_stack(&pool, hooks.clone());

    // Live-State (live) für channel.update + Session-Zuordnung.
    sqlx::query(
        "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_title, last_game, active_session_id)
         VALUES ('42', 'drag', 1, 'alt', 'Altspiel', 9)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Bits (inline, mit Session-Zuordnung über live_state).
    let bits = serde_json::json!({
        "subscription": {"type": "channel.cheer"},
        "event": {"broadcaster_user_id": "42", "user_login": "Fan", "bits": 250,
                   "message": "gg"}
    });
    let outcome = dispatcher
        .dispatch("channel.cheer", Some("m-b"), &bits)
        .await
        .unwrap();
    assert!(outcome.processed && !outcome.queued);
    let (donor, amount, session_id): (Option<String>, i32, Option<i64>) =
        sqlx::query_as("SELECT donor_login, amount, session_id FROM twitch_bits_events LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(donor.as_deref(), Some("fan"));
    assert_eq!(amount, 250);
    assert_eq!(session_id, Some(9));

    // Subscription-Gift.
    let gift = serde_json::json!({
        "subscription": {"type": "channel.subscription.gift"},
        "event": {"broadcaster_user_id": "42", "user_login": "gifter", "tier": "2000",
                   "is_gift": true, "total": 5}
    });
    dispatcher
        .dispatch("channel.subscription.gift", Some("m-g"), &gift)
        .await
        .unwrap();
    let (event_type, tier, total): (String, String, Option<i32>) = sqlx::query_as(
        "SELECT event_type, tier, total_gifted FROM twitch_subscription_events LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (event_type.as_str(), tier.as_str(), total),
        ("gift", "2000", Some(5))
    );

    // channel.update über die Inbox: Protokoll + Live-State-Update (nur live).
    let update = serde_json::json!({
        "subscription": {"type": "channel.update"},
        "event": {"broadcaster_user_id": "42", "broadcaster_user_login": "drag",
                   "title": "Neuer Titel", "category_name": "Deadlock"}
    });
    let outcome = dispatcher
        .dispatch("channel.update", Some("m-u"), &update)
        .await
        .unwrap();
    assert!(outcome.queued);
    assert!(wait_until_empty(&store).await);
    let (count, title): (i64, Option<String>) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM twitch_channel_updates),
                (SELECT last_title FROM twitch_live_state WHERE twitch_user_id = '42')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    assert_eq!(title.as_deref(), Some("Neuer Titel"));

    // channel.raid geht an den Hook, nicht in die Inbox.
    let raid = serde_json::json!({
        "event": {"to_broadcaster_user_id": "7", "to_broadcaster_user_login": "ziel",
                   "from_broadcaster_user_login": "drag", "viewers": 12}
    });
    let outcome = dispatcher
        .dispatch("channel.raid", Some("m-r"), &raid)
        .await
        .unwrap();
    assert!(outcome.processed && !outcome.queued);
    assert_eq!(hooks.channel_raid.load(Ordering::SeqCst), 1);

    // Unbekannter Typ → ok, aber nicht verarbeitet.
    let unknown = serde_json::json!({"event": {"broadcaster_user_id": "42"}});
    let outcome = dispatcher
        .dispatch("channel.unbekannt", Some("m-x"), &unknown)
        .await
        .unwrap();
    assert!(outcome.ok && !outcome.processed && !outcome.queued);

    runtime.shutdown().await;
}

// ── Hype-Train NULL-Bug (started_at nicht parsebar) ──────────────────────────

/// Normalfall: begin → end mit gültigem started_at → genau eine Zeile, phase='begin'
/// wird auf phase='end' aktualisiert (kein Doppel-INSERT).
#[tokio::test]
async fn hype_train_end_aktualisiert_begin_zeile() {
    let pool = pool_or_skip!("t4d_ht_normal");
    let store = TelemetryStore::new(pool.clone());

    let begin_event = serde_json::json!({
        "started_at": "2026-06-10T20:00:00Z",
        "level": 3,
        "total": 1200
    });
    store
        .store_hype_train_event("u1", &begin_event, HypeTrainPhase::Begin)
        .await
        .unwrap();

    let end_event = serde_json::json!({
        "started_at": "2026-06-10T20:00:00Z",
        "ended_at": "2026-06-10T20:05:00Z",
        "level": 4,
        "total": 1800
    });
    store
        .store_hype_train_event("u1", &end_event, HypeTrainPhase::End)
        .await
        .unwrap();

    let rows: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT event_phase, ended_at::text FROM twitch_hype_train_events ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    // Exakt eine Zeile — das Begin-Event wurde zum End-Event aktualisiert.
    assert_eq!(rows.len(), 1, "Kein Doppel-INSERT erwartet");
    assert_eq!(rows[0].0, "begin");
    assert!(rows[0].1.is_some(), "ended_at muss gesetzt sein");
}

/// Bug-Regression: started_at nicht parsebar (NULL) → End-Event darf nicht
/// als verwaiste Extra-Zeile landen. Stattdessen: ein INSERT mit phase='end'.
#[tokio::test]
async fn hype_train_end_mit_started_at_null_kein_verwaister_insert() {
    let pool = pool_or_skip!("t4d_ht_null");
    let store = TelemetryStore::new(pool.clone());

    // Kein vorheriges Begin-Event (simuliert den Fall, wo started_at fehlt/unlesbar ist).
    let end_event = serde_json::json!({
        // started_at fehlt absichtlich → None nach parse_dt_utc
        "ended_at": "2026-06-10T20:05:00Z",
        "level": 2,
        "total": 900
    });
    store
        .store_hype_train_event("u2", &end_event, HypeTrainPhase::End)
        .await
        .unwrap();

    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT event_phase FROM twitch_hype_train_events")
            .fetch_all(&pool)
            .await
            .unwrap();
    // Exakt ein Fallback-INSERT mit phase='end' — kein doppelter verwaister Eintrag.
    assert_eq!(rows.len(), 1, "Genau ein End-Insert erwartet");
    assert_eq!(rows[0].0, "end");
}
