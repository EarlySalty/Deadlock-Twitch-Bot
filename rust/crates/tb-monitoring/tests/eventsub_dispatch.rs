//! Hermetische Tests des EventSub-Ingress (Slice 4d): Dispatch-Dedup,
//! Inbox-Verarbeitung der Core-Typen, Telemetrie-Inserts, Offline-Throttle.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{Duration, Utc};
use sqlx::PgPool;
use std::sync::Mutex;
use tb_chat::TimeoutGuard;
use tb_monitoring::dispatch::ChatSubscriptionTelemetryHooks;
use tb_monitoring::poller::source::{ChannelInfo, ChannelInfoSource, SourceError};
use tb_monitoring::sessions::store::SessionStore;
use tb_monitoring::{
    epoch_clock, ChatNotificationKind, EventSubDispatcher, EventSubHooks, ExpSessionStore,
    ExpSessionTracker, GuardKind, GuardStore, HandlerError, HypeTrainPhase, InboxHandler,
    InboxRuntime, InboxRuntimeHandle, LiveStateStore, MonitoringEventHandler, NoFollowerSource,
    ProcessingInboxStore, SessionTracker, StreamSnapshot, TelemetryStore,
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
    /// Engagement-Auto-Off (läuft VOR dem Offline-Throttle, auch bei Duplikat).
    stream_offline_engagement: AtomicU64,
    /// Global-Ban-Sweep-Scheduling (läuft NACH dem Throttle, vor State-Finalize).
    stream_offline_global_ban: AtomicU64,
    channel_raid: AtomicU64,
    chat_raid: AtomicU64,
    chat_unraid: AtomicU64,
    /// Klassifizierte Sub-Notifications (in Reihenfolge des Eintreffens).
    chat_sub_kinds: Mutex<Vec<ChatNotificationKind>>,
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
    async fn on_stream_offline_engagement(&self, _twitch_user_id: &str, _login: Option<&str>) {
        self.stream_offline_engagement
            .fetch_add(1, Ordering::SeqCst);
    }
    async fn on_stream_offline_global_ban(&self, _twitch_user_id: &str, _login: Option<&str>) {
        self.stream_offline_global_ban
            .fetch_add(1, Ordering::SeqCst);
    }
    async fn on_stream_offline(&self, _twitch_user_id: &str, _login: Option<&str>) {
        self.stream_offline.fetch_add(1, Ordering::SeqCst);
    }
    async fn on_chat_subscription_notification(
        &self,
        kind: ChatNotificationKind,
        _event: &serde_json::Value,
        _message_id: Option<&str>,
    ) {
        self.chat_sub_kinds.lock().unwrap().push(kind);
    }
    async fn on_chat_raid_notification(
        &self,
        _event: &serde_json::Value,
        _message_id: Option<&str>,
    ) {
        self.chat_raid.fetch_add(1, Ordering::SeqCst);
    }
    async fn on_chat_unraid_notification(
        &self,
        _event: &serde_json::Value,
        _message_id: Option<&str>,
    ) {
        self.chat_unraid.fetch_add(1, Ordering::SeqCst);
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

struct PanicsOnceHandler {
    calls: AtomicU64,
}

#[async_trait::async_trait]
impl InboxHandler for PanicsOnceHandler {
    async fn handle(
        &self,
        _work_type: &str,
        _payload: &serde_json::Value,
    ) -> Result<(), HandlerError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("test handler panic");
        }
        Ok(())
    }
}

async fn wait_for_retry(
    store: &ProcessingInboxStore,
    work_type: &str,
) -> Option<tb_monitoring::PendingEntry> {
    for _ in 0..100 {
        let pending = store.list_pending(10).await.unwrap();
        if let Some(entry) = pending
            .into_iter()
            .find(|entry| entry.work_type == work_type && entry.attempt_count == 1)
        {
            return Some(entry);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    None
}

#[tokio::test]
async fn inbox_handler_panic_wird_retry_statt_worker_abort() {
    let pool = pool_or_skip!("t4d_inbox_panic_retry");
    let store = ProcessingInboxStore::new(pool.clone());
    let handler = Arc::new(PanicsOnceHandler {
        calls: AtomicU64::new(0),
    });
    let runtime = InboxRuntime::new(store.clone(), handler.clone())
        .with_clock(Arc::new(|| 100.0))
        .start();

    runtime
        .enqueue("panic.once", &serde_json::json!({"n": 1}), Some("panic-1"))
        .await
        .unwrap();
    let retry = wait_for_retry(&store, "panic.once")
        .await
        .expect("panic job should be marked retry");
    assert_eq!(retry.attempt_count, 1);
    assert!(retry
        .last_error
        .as_deref()
        .unwrap_or_default()
        .contains("panicked"));

    runtime
        .enqueue("after.panic", &serde_json::json!({"n": 2}), Some("ok-1"))
        .await
        .unwrap();
    for _ in 0..100 {
        let pending = store.list_pending(10).await.unwrap();
        if pending.len() == 1 && pending[0].work_type == "panic.once" {
            runtime.shutdown().await;
            assert_eq!(handler.calls.load(Ordering::SeqCst), 2);
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    runtime.shutdown().await;
    panic!("worker did not process the post-panic job");
}

#[tokio::test]
async fn core_notification_ohne_broadcaster_id_wird_fail_closed_abgelehnt() {
    let pool = pool_or_skip!("t4d_core_missing_broadcaster");
    let hooks = Arc::new(RecordingHooks::default());
    let (dispatcher, runtime, store) = build_stack(&pool, hooks);

    let malformed = serde_json::json!({
        "subscription": {"type": "stream.online"},
        "event": {"broadcaster_user_login": "drag"}
    });
    let err = dispatcher
        .dispatch("stream.online", Some("missing-broadcaster-1"), &malformed)
        .await
        .expect_err("Core-Delivery ohne broadcaster_id muss abgelehnt werden");
    assert!(err.to_string().contains("missing broadcaster_id"), "{err}");
    assert!(store.list_pending(10).await.unwrap().is_empty());
    runtime.shutdown().await;
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

#[tokio::test]
async fn stream_online_neuer_stream_leert_altes_announcement() {
    let pool = pool_or_skip!("t4d_online_resets_old_announcement");
    sqlx::query(
        "INSERT INTO twitch_live_state
            (twitch_user_id, streamer_login, is_live, last_stream_id,
             last_discord_message_id, last_tracking_token)
         VALUES ('42', 'drag', 0, 'old-stream', 'old-msg', 'old-token')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let hooks = Arc::new(RecordingHooks::default());
    let (dispatcher, runtime, store) = build_stack(&pool, hooks);
    let body = serde_json::json!({
        "subscription": {"type": "stream.online"},
        "event": {
            "broadcaster_user_id": "42",
            "broadcaster_user_login": "drag",
            "id": "new-stream",
            "started_at": "2026-06-09T17:00:00Z"
        }
    });

    dispatcher
        .dispatch("stream.online", Some("m-reset-1"), &body)
        .await
        .unwrap();
    assert!(wait_until_empty(&store).await, "Inbox nicht abgearbeitet");
    runtime.shutdown().await;

    #[derive(sqlx::FromRow)]
    struct StateRow {
        last_stream_id: Option<String>,
        last_discord_message_id: Option<String>,
        last_tracking_token: Option<String>,
    }
    let state: StateRow = sqlx::query_as(
        "SELECT last_stream_id, last_discord_message_id, last_tracking_token
           FROM twitch_live_state WHERE twitch_user_id = '42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(state.last_stream_id.as_deref(), Some("new-stream"));
    assert_eq!(state.last_discord_message_id, None);
    assert_eq!(state.last_tracking_token, None);
}

#[tokio::test]
async fn stream_online_gleicher_stream_respektiert_reconnect_fenster() {
    let pool = pool_or_skip!("t4d_online_same_stream_reconnect_window");
    sqlx::query(
        "INSERT INTO twitch_live_state
            (twitch_user_id, streamer_login, is_live, last_stream_id,
             last_discord_message_id, last_tracking_token)
         VALUES
            ('42', 'drag', 0, 'same-stream', 'keep-msg', 'keep-token'),
            ('43', 'flip', 0, 'same-stream', 'drop-msg', 'drop-token')",
    )
    .execute(&pool)
    .await
    .unwrap();
    GuardStore::new(pool.clone())
        .claim(
            GuardKind::BusinessEffect,
            "announcement_reannounce:drag",
            5.0 * 60.0,
            epoch_clock(),
        )
        .await
        .unwrap();
    GuardStore::new(pool.clone())
        .claim(
            GuardKind::BusinessEffect,
            "announcement_reannounce:flip",
            5.0 * 60.0,
            epoch_clock() - 6.0 * 60.0,
        )
        .await
        .unwrap();

    let hooks = Arc::new(RecordingHooks::default());
    let (dispatcher, runtime, store) = build_stack(&pool, hooks);
    for (user_id, login, message_id) in [
        ("42", "drag", "m-same-active"),
        ("43", "flip", "m-same-expired"),
    ] {
        let body = serde_json::json!({
            "subscription": {"type": "stream.online"},
            "event": {
                "broadcaster_user_id": user_id,
                "broadcaster_user_login": login,
                "id": "same-stream",
                "started_at": "2026-06-09T17:00:00Z"
            }
        });
        dispatcher
            .dispatch("stream.online", Some(message_id), &body)
            .await
            .unwrap();
    }
    assert!(wait_until_empty(&store).await, "Inbox nicht abgearbeitet");
    runtime.shutdown().await;

    let kept: Option<String> = sqlx::query_scalar(
        "SELECT last_discord_message_id FROM twitch_live_state WHERE twitch_user_id = '42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let dropped: Option<String> = sqlx::query_scalar(
        "SELECT last_discord_message_id FROM twitch_live_state WHERE twitch_user_id = '43'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(kept.as_deref(), Some("keep-msg"));
    assert_eq!(dropped, None);
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
    let (ended, notes): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT ended_at::text, notes FROM twitch_stream_sessions LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(ended.is_some());
    assert_eq!(notes.as_deref(), Some("offline"));
    assert_eq!(hooks.stream_offline.load(Ordering::SeqCst), 1);
    // Erster Offline: Engagement (vor Throttle) + Global-Ban (nach Throttle) laufen.
    assert_eq!(hooks.stream_offline_engagement.load(Ordering::SeqCst), 1);
    assert_eq!(hooks.stream_offline_global_ban.load(Ordering::SeqCst), 1);

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
        "Offline-Throttle verhindert Doppel-Trigger (Auto-Raid)"
    );
    // B5-09 Parität: Engagement-Auto-Off läuft VOR dem Throttle und damit auch
    // beim gedrosselten Duplikat (Python `eventsub_mixin.py`:1861).
    assert_eq!(
        hooks.stream_offline_engagement.load(Ordering::SeqCst),
        2,
        "Engagement-Auto-Off läuft auch beim gedrosselten Duplikat"
    );
    // Global-Ban-Sweep läuft NACH dem Throttle → nur einmal (kein Duplikat).
    assert_eq!(
        hooks.stream_offline_global_ban.load(Ordering::SeqCst),
        1,
        "Global-Ban-Sweep nur nach bestandenem Throttle"
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

    // channel.raid läuft durable über die Inbox; der Handler ruft den Hook.
    let raid = serde_json::json!({
        "event": {"to_broadcaster_user_id": "7", "to_broadcaster_user_login": "ziel",
                   "from_broadcaster_user_login": "drag", "viewers": 12}
    });
    let outcome = dispatcher
        .dispatch("channel.raid", Some("m-r"), &raid)
        .await
        .unwrap();
    assert!(outcome.queued && !outcome.processed);
    assert!(wait_until_empty(&store).await);
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

#[tokio::test]
async fn stream_online_gibt_offline_throttle_frei() {
    let pool = pool_or_skip!("t4d_offline_throttle_release");
    let hooks = Arc::new(RecordingHooks::default());
    let (dispatcher, runtime, store) = build_stack(&pool, hooks.clone());

    sqlx::query(
        "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live)
         VALUES ('42', 'drag', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let offline = serde_json::json!({
        "subscription": {"type": "stream.offline"},
        "event": {"broadcaster_user_id": "42", "broadcaster_user_login": "drag"}
    });
    let online = serde_json::json!({
        "subscription": {"type": "stream.online"},
        "event": {
            "broadcaster_user_id": "42",
            "broadcaster_user_login": "drag",
            "id": "s-2",
            "started_at": "2026-06-10T12:00:00Z"
        }
    });

    dispatcher
        .dispatch("stream.offline", Some("flap-off-1"), &offline)
        .await
        .unwrap();
    assert!(wait_until_empty(&store).await);
    assert_eq!(hooks.stream_offline.load(Ordering::SeqCst), 1);

    dispatcher
        .dispatch("stream.online", Some("flap-on-1"), &online)
        .await
        .unwrap();
    assert!(wait_until_empty(&store).await);

    dispatcher
        .dispatch("stream.offline", Some("flap-off-2"), &offline)
        .await
        .unwrap();
    assert!(wait_until_empty(&store).await);
    runtime.shutdown().await;

    assert_eq!(
        hooks.stream_offline.load(Ordering::SeqCst),
        2,
        "Go-Live muss den stale OfflineThrottle-Guard freigeben"
    );
}

#[tokio::test]
async fn channel_ban_bot_self_timeout_armt_timeout_guard() {
    let pool = pool_or_skip!("t4d_ban_timeout_guard");
    let guard = Arc::new(TimeoutGuard::new());
    let store = TelemetryStore::new(pool.clone()).with_bot_timeout_guard("bot-42", guard.clone());

    let timeout = serde_json::json!({
        "broadcaster_user_login": "Drag",
        "user_id": "bot-42",
        "user_login": "bot",
        "moderator_user_login": "mod",
        "reason": "timeout",
        "ends_at": "2026-06-10T12:01:00Z"
    });
    store
        .store_ban_event("42", Some("drag"), &timeout, false, Utc::now())
        .await
        .unwrap();
    assert!(
        guard.consume_stream_start_pitch("drag"),
        "non-permanent bot self-ban muss record_timeout(login) ausloesen"
    );

    let permanent = serde_json::json!({
        "broadcaster_user_login": "Drag",
        "user_id": "bot-42",
        "user_login": "bot",
        "moderator_user_login": "mod",
        "reason": "ban"
    });
    store
        .store_ban_event("42", Some("drag"), &permanent, false, Utc::now())
        .await
        .unwrap();
    assert!(
        !guard.consume_stream_start_pitch("drag"),
        "permanenter Ban darf keinen TimeoutGuard-Eintrag erzeugen"
    );
}

#[tokio::test]
async fn channel_moderate_unban_speichert_unban_und_entfernt_global_ban_applied() {
    let pool = pool_or_skip!("t4d_moderate_unban_global_applied");
    let hooks = Arc::new(RecordingHooks::default());
    let (dispatcher, runtime, _store) = build_stack(&pool, hooks);

    sqlx::query(
        "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id)
         VALUES ('helmbombenricky', '147713656')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_chatter_global_ban_applied (chatter_login, broadcaster_id)
         VALUES ('helmbombenricky', '58819840')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let payload = serde_json::json!({
        "subscription": {"type": "channel.moderate"},
        "event": {
            "broadcaster_user_id": "58819840",
            "broadcaster_user_login": "ismile_e",
            "action": "unban",
            "moderator_user_login": "ismile_e",
            "unban": {
                "user_login": "helmbombenricky",
                "user_id": "147713656"
            }
        }
    });
    let outcome = dispatcher
        .dispatch("channel.moderate", Some("m-moderate-unban"), &payload)
        .await
        .unwrap();
    runtime.shutdown().await;
    assert!(outcome.processed && !outcome.queued);

    let events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_ban_events
         WHERE twitch_user_id = '58819840'
           AND event_type = 'unban'
           AND target_login = 'helmbombenricky'
           AND target_id = '147713656'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let applied: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_chatter_global_ban_applied
         WHERE broadcaster_id = '58819840'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(events, 1);
    assert_eq!(applied, 0, "Unban macht den nächsten Sweep wieder fällig");
}

#[tokio::test]
async fn monitoring_handler_gibt_unknown_work_type_als_fehler_zurueck() {
    let pool = pool_or_skip!("t4d_unknown_work_type");
    let guard = GuardStore::new(pool.clone());
    let live_state = LiveStateStore::new(pool.clone());
    let tracker = Arc::new(SessionTracker::new(
        SessionStore::new(pool.clone()),
        live_state.clone(),
        ExpSessionTracker::new(ExpSessionStore::new(pool.clone())),
        Arc::new(NoFollowerSource),
        "Deadlock",
    ));
    let handler = MonitoringEventHandler::new(
        guard,
        live_state,
        tracker,
        TelemetryStore::new(pool),
        Arc::new(RecordingHooks::default()),
        None,
        Arc::new(epoch_clock),
    );

    let err = handler
        .handle("future.eventsub.type", &serde_json::json!({"event": {}}))
        .await
        .expect_err("unbekannter Work-Type muss den Inbox-Retry-Pfad triggern");
    assert!(err
        .to_string()
        .contains("unknown eventsub processing work_type"));
}

#[tokio::test]
async fn monitoring_handler_verarbeitet_stream_online_followups_work_type() {
    let pool = pool_or_skip!("t4d_stream_online_followups");
    let guard = GuardStore::new(pool.clone());
    let live_state = LiveStateStore::new(pool.clone());
    let tracker = Arc::new(SessionTracker::new(
        SessionStore::new(pool.clone()),
        live_state.clone(),
        ExpSessionTracker::new(ExpSessionStore::new(pool.clone())),
        Arc::new(NoFollowerSource),
        "Deadlock",
    ));
    let hooks = Arc::new(RecordingHooks::default());
    let handler = MonitoringEventHandler::new(
        guard,
        live_state,
        tracker,
        TelemetryStore::new(pool),
        hooks.clone(),
        None,
        Arc::new(epoch_clock),
    );

    handler
        .handle(
            "stream.online.followups",
            &serde_json::json!({
                "broadcaster_user_id": "42",
                "broadcaster_login": "Drag",
                "login_value": "drag",
                "stream_id": "s-2",
                "message_id": "m-followups-1"
            }),
        )
        .await
        .unwrap();

    assert_eq!(hooks.went_live.load(Ordering::SeqCst), 1);
    assert_eq!(hooks.score_refresh.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn chat_subscription_telemetry_hook_persistiert_mit_should_capture_gate() {
    let pool = pool_or_skip!("t4d_chat_sub_telemetry_hook");
    let has_dedicated_sub = Arc::new(AtomicBool::new(false));
    let checker = {
        let has_dedicated_sub = has_dedicated_sub.clone();
        Arc::new(move |eventsub_type: &str, broadcaster_id: &str| {
            assert_eq!(eventsub_type, "channel.subscribe");
            assert_eq!(broadcaster_id, "42");
            has_dedicated_sub.load(Ordering::SeqCst)
        })
    };
    let hooks = ChatSubscriptionTelemetryHooks::new(
        Arc::new(RecordingHooks::default()),
        TelemetryStore::new(pool.clone()),
        checker,
        Arc::new(|| 1_783_000_000.0),
    );
    let event = serde_json::json!({
        "broadcaster_user_id": "42",
        "notice_type": "sub",
        "chatter_user_login": "Fan",
        "chatter_user_id": "fan-1",
        "sub": {"sub_tier": "1000"}
    });

    hooks
        .on_chat_subscription_notification(ChatNotificationKind::Sub, &event, Some("cn-1"))
        .await;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_subscription_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);

    has_dedicated_sub.store(true, Ordering::SeqCst);
    hooks
        .on_chat_subscription_notification(ChatNotificationKind::Sub, &event, Some("cn-2"))
        .await;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_subscription_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        count, 1,
        "dedizierte EventSub aktiv: Chat-Fallback darf nicht doppelt zaehlen"
    );
}

/// B8-00: `channel.chat.notification` demuxt nach `notice_type` an die
/// Routing-Punkte — sub/resub/sub_gift/community_sub_gift → Sub-Hook,
/// raid/unraid → Raid-Hooks; unbekannter notice_type wird sauber ignoriert
/// (kein Panic, nicht „processed"). Foundation für B8-01 (Sub-Telemetrie) und
/// B7 (Raid-Korrelation).
#[tokio::test]
async fn chat_notification_demuxt_nach_notice_type() {
    let pool = pool_or_skip!("t4d_chat_notification");
    let hooks = Arc::new(RecordingHooks::default());
    let (dispatcher, runtime, _store) = build_stack(&pool, hooks.clone());

    // notice_type=sub → Sub-Hook (B8-01), processed.
    let sub = serde_json::json!({
        "subscription": {"type": "channel.chat.notification"},
        "event": {"broadcaster_user_id": "42", "notice_type": "sub",
                   "chatter_user_login": "fan", "sub": {"sub_tier": "1000"}}
    });
    let outcome = dispatcher
        .dispatch("channel.chat.notification", Some("cn-sub"), &sub)
        .await
        .unwrap();
    assert!(outcome.processed && !outcome.queued);

    // raid + unraid → Raid-Hooks (B7).
    for (mid, notice) in [("cn-raid", "raid"), ("cn-unraid", "unraid")] {
        let body = serde_json::json!({
            "subscription": {"type": "channel.chat.notification"},
            "event": {"broadcaster_user_id": "42", "notice_type": notice}
        });
        let outcome = dispatcher
            .dispatch("channel.chat.notification", Some(mid), &body)
            .await
            .unwrap();
        assert!(outcome.processed, "{notice} muss geroutet werden");
    }

    // community_sub_gift → Sub-Hook (Batch-Geschenk).
    let community = serde_json::json!({
        "subscription": {"type": "channel.chat.notification"},
        "event": {"broadcaster_user_id": "42", "notice_type": "community_sub_gift",
                   "community_sub_gift": {"total": 5, "sub_tier": "1000"}}
    });
    dispatcher
        .dispatch("channel.chat.notification", Some("cn-comm"), &community)
        .await
        .unwrap();

    // Unbekannter notice_type → kein Panic, nicht processed.
    let unknown = serde_json::json!({
        "subscription": {"type": "channel.chat.notification"},
        "event": {"broadcaster_user_id": "42", "notice_type": "announcement"}
    });
    let outcome = dispatcher
        .dispatch("channel.chat.notification", Some("cn-unknown"), &unknown)
        .await
        .unwrap();
    assert!(outcome.ok && !outcome.processed && !outcome.queued);

    runtime.shutdown().await;

    assert_eq!(hooks.chat_raid.load(Ordering::SeqCst), 1);
    assert_eq!(hooks.chat_unraid.load(Ordering::SeqCst), 1);
    let sub_kinds = hooks.chat_sub_kinds.lock().unwrap().clone();
    assert_eq!(
        sub_kinds,
        vec![
            ChatNotificationKind::Sub,
            ChatNotificationKind::CommunitySubGift
        ],
        "Sub-Notices in Reihenfolge an den Sub-Routing-Punkt"
    );
}

/// 65.3 Readiness-Gate: `ensure_dispatch_ready` lehnt VOR dem Dispatch ab,
/// wenn (a) der Dispatch deaktiviert ist oder (b) kein Handler für den Sub-Typ
/// registriert ist. Beide Fälle → der Webhook-Receiver antwortet 503.
#[tokio::test]
async fn ensure_dispatch_ready_gate() {
    use tb_monitoring::DispatchNotReady;

    let pool = pool_or_skip!("t4d_ready_gate");
    let hooks = Arc::new(RecordingHooks::default());
    let (dispatcher, runtime, _store) = build_stack(&pool, hooks.clone());

    // Default: aktiv → registrierter Typ passiert das Gate.
    assert!(dispatcher.is_dispatch_active());
    assert_eq!(dispatcher.ensure_dispatch_ready("stream.online"), Ok(()));
    // Normalisierung greift auch im Gate.
    assert_eq!(dispatcher.ensure_dispatch_ready("  Channel.Raid  "), Ok(()));

    // Unbekannter Sub-Typ → CallbackNotRegistered (Python
    // EventSubCallbackNotRegistered), Receiver → 503.
    assert_eq!(
        dispatcher.ensure_dispatch_ready("channel.unbekannt"),
        Err(DispatchNotReady::CallbackNotRegistered(
            "channel.unbekannt".into()
        ))
    );

    // Deaktiviert → jede Notification scheitert am Aktiv-Check, auch eine, die
    // sonst einen Handler hätte (Python `_notification_dispatch_active`).
    dispatcher.set_dispatch_active(false);
    assert!(!dispatcher.is_dispatch_active());
    assert_eq!(
        dispatcher.ensure_dispatch_ready("stream.online"),
        Err(DispatchNotReady::DispatchInactive)
    );

    // Wieder aktiv → Gate offen.
    dispatcher.set_dispatch_active(true);
    assert_eq!(dispatcher.ensure_dispatch_ready("stream.online"), Ok(()));

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

    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT event_phase, ended_at::text FROM twitch_hype_train_events ORDER BY id",
    )
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

    let rows: Vec<(String,)> = sqlx::query_as("SELECT event_phase FROM twitch_hype_train_events")
        .fetch_all(&pool)
        .await
        .unwrap();
    // Exakt ein Fallback-INSERT mit phase='end' — kein doppelter verwaister Eintrag.
    assert_eq!(rows.len(), 1, "Genau ein End-Insert erwartet");
    assert_eq!(rows[0].0, "end");
}
