//! Tests des Subscription-Lifecycles (Slice 4d-ii): Ensure mit Tracking-Dedup,
//! Cleanup nur für eigene Callback-URL + inaktive Ziele, Capacity-Snapshot.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use tb_monitoring::poller::source::SourceError;
use tb_monitoring::{
    CapacitySnapshotStore, RemoteSubscription, SubscriptionConfig, SubscriptionManager,
    SubscriptionTransport,
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

/// Stub-Backend: zeichnet Creates/Deletes auf, liefert programmierte Listen.
#[derive(Default)]
struct StubTransport {
    creates: Mutex<Vec<(String, String)>>,
    conditions: Mutex<Vec<(String, serde_json::Value)>>,
    /// (sub_type, version) — prüft den Versions-Pfad (z. B. channel.follow v2).
    versions: Mutex<Vec<(String, String)>>,
    /// (sub_type, bearer_override) — prüft den Telemetrie-Token-Pfad.
    bearers: Mutex<Vec<(String, Option<String>)>>,
    deletes: Mutex<Vec<String>>,
    listing: Mutex<Vec<RemoteSubscription>>,
}

#[async_trait::async_trait]
impl SubscriptionTransport for StubTransport {
    async fn create(
        &self,
        sub_type: &str,
        version: &str,
        condition: &serde_json::Value,
        _callback: &str,
        _secret: &str,
        bearer_override: Option<&str>,
    ) -> Result<bool, SourceError> {
        self.versions
            .lock()
            .unwrap()
            .push((sub_type.to_string(), version.to_string()));
        let bid = condition
            .get("broadcaster_user_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        self.creates
            .lock()
            .unwrap()
            .push((sub_type.to_string(), bid));
        self.conditions
            .lock()
            .unwrap()
            .push((sub_type.to_string(), condition.clone()));
        self.bearers
            .lock()
            .unwrap()
            .push((sub_type.to_string(), bearer_override.map(str::to_string)));
        Ok(false)
    }
    async fn list(&self) -> Result<Vec<RemoteSubscription>, SourceError> {
        Ok(self.listing.lock().unwrap().clone())
    }
    async fn delete(&self, id: &str) -> Result<(), SourceError> {
        self.deletes.lock().unwrap().push(id.to_string());
        Ok(())
    }
}

fn sub(id: &str, sub_type: &str, callback: &str, bid: &str) -> RemoteSubscription {
    RemoteSubscription {
        id: id.to_string(),
        sub_type: sub_type.to_string(),
        status: "enabled".to_string(),
        callback: Some(callback.to_string()),
        broadcaster_user_id: Some(bid.to_string()),
    }
}

#[tokio::test]
async fn ensure_dedupliziert_und_schreibt_capacity_snapshot() {
    let pool = pool_or_skip!("t4d_subs_ensure");
    let transport = Arc::new(StubTransport::default());
    let manager = SubscriptionManager::new(
        transport.clone(),
        SubscriptionConfig {
            callback_url: "https://cb/x".to_string(),
            secret: "geheim".to_string(),
        },
        CapacitySnapshotStore::new(pool.clone()),
    );

    assert!(manager.ensure_offline_subscription("42", "drag").await);
    // Zweiter Aufruf: in-memory getrackt → kein zweiter Create.
    assert!(manager.ensure_offline_subscription("42", "drag").await);
    assert_eq!(transport.creates.lock().unwrap().len(), 1);
    // Leere broadcaster_id → kein Create.
    assert!(!manager.ensure_offline_subscription("  ", "x").await);

    // stream.offline-Subscribe löst einen Capacity-Snapshot aus.
    let (trigger, used): (String, i32) = sqlx::query_as(
        "SELECT trigger_reason, used_slots FROM twitch_eventsub_capacity_snapshot
          ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(trigger, "stream_offline_subscribed");
    assert_eq!(used, 1);

    // Core-Subscriptions: drei Typen pro Broadcaster.
    manager.ensure_core_subscriptions("77", "neu").await;
    let creates = transport.creates.lock().unwrap();
    let for_77: Vec<&str> = creates
        .iter()
        .filter(|(_, bid)| bid == "77")
        .map(|(t, _)| t.as_str())
        .collect();
    assert_eq!(
        for_77,
        vec!["stream.online", "stream.offline", "channel.update"]
    );
}

#[tokio::test]
async fn rehydrate_und_cleanup_nur_eigene_callback() {
    let pool = pool_or_skip!("t4d_subs_cleanup");
    let transport = Arc::new(StubTransport::default());
    *transport.listing.lock().unwrap() = vec![
        sub("a", "stream.offline", "https://cb/x", "42"), // aktiv → bleibt
        sub("b", "stream.offline", "https://cb/x", "99"), // inaktiv → weg
        sub("c", "stream.offline", "https://anderes/cb", "99"), // fremde URL → bleibt
    ];
    let manager = SubscriptionManager::new(
        transport.clone(),
        SubscriptionConfig {
            callback_url: "https://cb/x".to_string(),
            secret: "geheim".to_string(),
        },
        CapacitySnapshotStore::new(pool.clone()),
    );

    manager.rehydrate().await;
    // Rehydriert: stream.offline/42 getrackt → ensure macht keinen Create.
    assert!(manager.ensure_offline_subscription("42", "drag").await);
    assert!(transport.creates.lock().unwrap().is_empty());

    let active: HashSet<String> = ["42".to_string()].into_iter().collect();
    assert_eq!(manager.cleanup_stale(&active).await, 1);
    assert_eq!(*transport.deletes.lock().unwrap(), vec!["b".to_string()]);
}

#[tokio::test]
async fn raid_subscription_nutzt_to_broadcaster_condition_und_dedup() {
    let pool = pool_or_skip!("t6_subs_raid");
    let transport = Arc::new(StubTransport::default());
    let manager = SubscriptionManager::new(
        transport.clone(),
        SubscriptionConfig {
            callback_url: "https://cb/x".to_string(),
            secret: "geheim".to_string(),
        },
        CapacitySnapshotStore::new(pool.clone()),
    );

    assert!(manager.ensure_raid_subscription("777", "ziel").await);
    // channel.raid wird über das RAID-ZIEL abonniert, nicht den Broadcaster.
    let conditions = transport.conditions.lock().unwrap().clone();
    assert_eq!(conditions.len(), 1);
    assert_eq!(conditions[0].0, "channel.raid");
    assert_eq!(
        conditions[0].1,
        serde_json::json!({ "to_broadcaster_user_id": "777" })
    );
    drop(conditions);

    // Dedup über Tracking; leere ID → kein Create.
    assert!(manager.ensure_raid_subscription("777", "ziel").await);
    assert_eq!(transport.conditions.lock().unwrap().len(), 1);
    assert!(!manager.ensure_raid_subscription(" ", "x").await);
}

#[tokio::test]
async fn broadcaster_telemetry_subs_scope_gefiltert_und_mit_bearer() {
    let pool = pool_or_skip!("t9_subs_telemetry");
    let transport = Arc::new(StubTransport::default());
    let manager = SubscriptionManager::new(
        transport.clone(),
        SubscriptionConfig {
            callback_url: "https://cb/x".to_string(),
            secret: "geheim".to_string(),
        },
        CapacitySnapshotStore::new(pool.clone()),
    );

    // Token hat nur bits:read + channel:read:subscriptions → 2 Bits-Subs
    // (cheer/bits.use) + 4 Subscription-Subs werden angelegt, Hype/Ads/Points
    // mangels Scope übersprungen.
    let scopes = vec![
        "bits:read".to_string(),
        "channel:read:subscriptions".to_string(),
    ];
    let ensured = manager
        .ensure_broadcaster_telemetry_subscriptions("555", "partner", "BROADCASTERTOKEN", &scopes)
        .await;
    assert_eq!(ensured, 6);

    let creates = transport.creates.lock().unwrap().clone();
    let mut types: Vec<&str> = creates
        .iter()
        .filter(|(_, bid)| bid == "555")
        .map(|(t, _)| t.as_str())
        .collect();
    types.sort_unstable();
    assert_eq!(
        types,
        vec![
            "channel.bits.use",
            "channel.cheer",
            "channel.subscribe",
            "channel.subscription.end",
            "channel.subscription.gift",
            "channel.subscription.message",
        ]
    );
    // Hype-Train wurde mangels Scope nicht versucht.
    assert!(!creates.iter().any(|(t, _)| t.starts_with("channel.hype_train")));

    // Jeder Telemetrie-Create lief mit dem Broadcaster-Token als Bearer.
    let bearers = transport.bearers.lock().unwrap().clone();
    assert!(bearers
        .iter()
        .all(|(_, b)| b.as_deref() == Some("BROADCASTERTOKEN")));

    // Zweiter Aufruf: alles getrackt → kein neuer Create.
    let again = manager
        .ensure_broadcaster_telemetry_subscriptions("555", "partner", "BROADCASTERTOKEN", &scopes)
        .await;
    assert_eq!(again, 6);
    assert_eq!(transport.creates.lock().unwrap().len(), 6);

    // Leerer Token / leere ID → kein Create.
    assert_eq!(
        manager
            .ensure_broadcaster_telemetry_subscriptions("555", "p", "  ", &scopes)
            .await,
        0
    );
    assert_eq!(
        manager
            .ensure_broadcaster_telemetry_subscriptions(" ", "p", "tok", &scopes)
            .await,
        0
    );
}

#[tokio::test]
async fn first_message_sub_nutzt_bot_token_und_user_id_condition() {
    let pool = pool_or_skip!("b5_subs_first_message");
    let transport = Arc::new(StubTransport::default());
    let manager = SubscriptionManager::new(
        transport.clone(),
        SubscriptionConfig {
            callback_url: "https://cb/x".to_string(),
            secret: "geheim".to_string(),
        },
        CapacitySnapshotStore::new(pool.clone()),
    );

    // channel.chat.user_first_message: Condition {broadcaster_user_id, user_id:<bot>},
    // Auth = Bot-Token (Python eventsub_mixin.py:2692).
    assert!(
        manager
            .ensure_first_message_subscription("555", "BOTID", "BOTTOKEN", "partner")
            .await
    );
    let conditions = transport.conditions.lock().unwrap().clone();
    assert_eq!(conditions.len(), 1);
    assert_eq!(conditions[0].0, "channel.chat.user_first_message");
    assert_eq!(
        conditions[0].1,
        serde_json::json!({ "broadcaster_user_id": "555", "user_id": "BOTID" })
    );
    drop(conditions);

    let bearers = transport.bearers.lock().unwrap().clone();
    assert_eq!(bearers, vec![("channel.chat.user_first_message".to_string(), Some("BOTTOKEN".to_string()))]);
    drop(bearers);

    // Zweiter Aufruf: getrackt → kein neuer Create.
    assert!(
        manager
            .ensure_first_message_subscription("555", "BOTID", "BOTTOKEN", "partner")
            .await
    );
    assert_eq!(transport.creates.lock().unwrap().len(), 1);

    // Leere ID / leerer Bot-Token / leere Bot-ID → kein Create.
    assert!(!manager.ensure_first_message_subscription(" ", "BOTID", "BOTTOKEN", "p").await);
    assert!(!manager.ensure_first_message_subscription("555", "BOTID", "  ", "p").await);
    assert!(!manager.ensure_first_message_subscription("555", " ", "BOTTOKEN", "p").await);
    assert_eq!(transport.creates.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn moderator_telemetry_subs_scope_gefiltert_mit_bot_token_und_moderator_id() {
    let pool = pool_or_skip!("b5_subs_mod_telemetry");
    let transport = Arc::new(StubTransport::default());
    let manager = SubscriptionManager::new(
        transport.clone(),
        SubscriptionConfig {
            callback_url: "https://cb/x".to_string(),
            secret: "geheim".to_string(),
        },
        CapacitySnapshotStore::new(pool.clone()),
    );

    // Bot-Token mit moderator:read:followers + moderator:manage:banned_users →
    // channel.follow + channel.ban + channel.unban, KEIN shoutout (Scope fehlt).
    let scopes = vec![
        "moderator:read:followers".to_string(),
        "moderator:manage:banned_users".to_string(),
    ];
    let ensured = manager
        .ensure_moderator_telemetry_subscriptions("555", "BOTID", "BOTTOKEN", &scopes, "partner")
        .await;
    assert_eq!(ensured, 3);

    let creates = transport.creates.lock().unwrap().clone();
    let mut types: Vec<&str> = creates
        .iter()
        .filter(|(_, bid)| bid == "555")
        .map(|(t, _)| t.as_str())
        .collect();
    types.sort_unstable();
    assert_eq!(types, vec!["channel.ban", "channel.follow", "channel.unban"]);
    // Shoutout mangels Scope nicht versucht.
    assert!(!creates.iter().any(|(t, _)| t.starts_with("channel.shoutout")));
    drop(creates);

    // Alle Condition tragen broadcaster_user_id + moderator_user_id:<bot>.
    let conditions = transport.conditions.lock().unwrap().clone();
    for (sub_type, condition) in &conditions {
        assert_eq!(
            condition,
            &serde_json::json!({ "broadcaster_user_id": "555", "moderator_user_id": "BOTID" }),
            "{sub_type} hat falsche Condition"
        );
    }
    drop(conditions);

    // channel.follow nutzt Version 2 (Twitch-Vertrag).
    let versions = transport.versions.lock().unwrap().clone();
    let follow_version = versions
        .iter()
        .find(|(t, _)| t == "channel.follow")
        .map(|(_, v)| v.as_str());
    assert_eq!(follow_version, Some("2"));
    drop(versions);

    // Jeder Create lief mit dem Bot-Token als Bearer.
    let bearers = transport.bearers.lock().unwrap().clone();
    assert!(bearers.iter().all(|(_, b)| b.as_deref() == Some("BOTTOKEN")));
    drop(bearers);

    // Zweiter Aufruf: getrackt → kein neuer Create.
    let again = manager
        .ensure_moderator_telemetry_subscriptions("555", "BOTID", "BOTTOKEN", &scopes, "partner")
        .await;
    assert_eq!(again, 3);
    assert_eq!(transport.creates.lock().unwrap().len(), 3);

    // Leere ID / leerer Token / leere Bot-ID → kein Create.
    assert_eq!(
        manager
            .ensure_moderator_telemetry_subscriptions(" ", "BOTID", "BOTTOKEN", &scopes, "p")
            .await,
        0
    );
    assert_eq!(
        manager
            .ensure_moderator_telemetry_subscriptions("555", "BOTID", "  ", &scopes, "p")
            .await,
        0
    );
    assert_eq!(
        manager
            .ensure_moderator_telemetry_subscriptions("555", " ", "BOTTOKEN", &scopes, "p")
            .await,
        0
    );
}

// ── B8-07-RECONCILE: Passive-Lurker-Gate vor dem Chat-Subscribe ──────────────

#[tokio::test]
async fn chat_subscribe_passiver_lurker_schreibt_state_statt_zu_subscriben() {
    let pool = pool_or_skip!("b8_07_chat_lurker");
    let transport = Arc::new(StubTransport::default());
    let manager = SubscriptionManager::new(
        transport.clone(),
        SubscriptionConfig {
            callback_url: "https://cb/x".to_string(),
            secret: "geheim".to_string(),
        },
        CapacitySnapshotStore::new(pool.clone()),
    );

    // Monitored-only-Kanal OHNE Partner-State und OHNE Raid-Auth → passiver Lurker.
    sqlx::query(
        "INSERT INTO twitch_streamers (twitch_login, twitch_user_id, is_monitored_only) \
         VALUES ('lurker', '900', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Treffer → kein Subscribe-Versuch, State = passive_lurker.
    assert!(
        !manager
            .ensure_chat_subscriptions("900", "BOTID", "lurker")
            .await
    );
    assert!(
        transport.creates.lock().unwrap().is_empty(),
        "passiver Lurker darf keinen Subscribe-Versuch auslösen"
    );

    // Beide Chat-Sub-Typen tragen den Lurker-State + Detail (1:1 Python).
    let states = manager.chat_subscription_states("lurker");
    let mut keys: Vec<&str> = states.iter().map(|(t, _, _)| t.as_str()).collect();
    keys.sort_unstable();
    assert_eq!(keys, vec!["channel.chat.message", "channel.chat.notification"]);
    for (_, state, detail) in &states {
        assert_eq!(state, tb_chat::PASSIVE_LURKER_STATE);
        assert_eq!(detail.as_deref(), Some(tb_chat::PASSIVE_LURKER_DETAIL));
    }
}

#[tokio::test]
async fn chat_subscribe_aktiver_partner_subscribed_normal() {
    let pool = pool_or_skip!("b8_07_chat_partner");
    let transport = Arc::new(StubTransport::default());
    let manager = SubscriptionManager::new(
        transport.clone(),
        SubscriptionConfig {
            callback_url: "https://cb/x".to_string(),
            secret: "geheim".to_string(),
        },
        CapacitySnapshotStore::new(pool.clone()),
    );

    // Monitored-only, ABER aktiver Partner → kein Lurker (is_partner_active=1).
    sqlx::query(
        "INSERT INTO twitch_streamers (twitch_login, twitch_user_id, is_monitored_only) \
         VALUES ('partner', '901', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_streamers_partner_state \
            (twitch_login, twitch_user_id, is_partner_active) \
         VALUES ('partner', '901', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Kein Lurker → normaler Subscribe (beide Chat-Sub-Typen).
    assert!(
        manager
            .ensure_chat_subscriptions("901", "BOTID", "partner")
            .await
    );
    let creates = transport.creates.lock().unwrap().clone();
    let mut types: Vec<&str> = creates.iter().map(|(t, _)| t.as_str()).collect();
    types.sort_unstable();
    assert_eq!(types, vec!["channel.chat.message", "channel.chat.notification"]);
    // Kein Lurker-State geschrieben.
    assert!(manager.chat_subscription_states("partner").is_empty());
}

#[tokio::test]
async fn chat_subscribe_lurker_mit_raid_auth_subscribed_normal() {
    let pool = pool_or_skip!("b8_07_chat_raidauth");
    let transport = Arc::new(StubTransport::default());
    let manager = SubscriptionManager::new(
        transport.clone(),
        SubscriptionConfig {
            callback_url: "https://cb/x".to_string(),
            secret: "geheim".to_string(),
        },
        CapacitySnapshotStore::new(pool.clone()),
    );

    // Monitored-only, kein Partner, ABER Raid-Auth vorhanden → kein Lurker.
    sqlx::query(
        "INSERT INTO twitch_streamers (twitch_login, twitch_user_id, is_monitored_only) \
         VALUES ('raider', '902', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login) VALUES ('902', 'raider')",
    )
    .execute(&pool)
    .await
    .unwrap();

    assert!(
        manager
            .ensure_chat_subscriptions("902", "BOTID", "raider")
            .await
    );
    assert_eq!(transport.creates.lock().unwrap().len(), 2);
    assert!(manager.chat_subscription_states("raider").is_empty());
}

/// Manuell vorrückbare Test-Uhr (Epoch-Sekunden) für die Capacity-Throttle-Fenster.
fn fake_clock() -> (Arc<AtomicU64>, tb_monitoring::ClockFn) {
    let now = Arc::new(AtomicU64::new(0));
    let handle = now.clone();
    let clock: tb_monitoring::ClockFn = Arc::new(move || handle.load(Ordering::SeqCst) as f64);
    (now, clock)
}

async fn snapshot_count(pool: &sqlx::PgPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM twitch_eventsub_capacity_snapshot")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn periodic_capacity_snapshot_throttelt_auf_sample_intervall() {
    let pool = pool_or_skip!("b5_08_capacity_periodic");
    let transport = Arc::new(StubTransport::default());
    let (clock_now, clock) = fake_clock();
    let manager = SubscriptionManager::new(
        transport.clone(),
        SubscriptionConfig {
            callback_url: "https://cb/x".to_string(),
            secret: "geheim".to_string(),
        },
        CapacitySnapshotStore::new(pool.clone()),
    )
    .with_clock(clock);

    // Zwei getrackte Subs → used_slots = 2.
    manager.ensure_offline_subscription("11", "a").await;
    manager.ensure_offline_subscription("22", "b").await;
    // ensure_offline_subscription hat bereits zwei "stream_offline_subscribed"-Zeilen
    // geschrieben; nur die periodischen Zeilen interessieren hier.
    let base = snapshot_count(&pool).await;

    // Erster periodischer Aufruf bei t=0 schreibt immer.
    manager.record_capacity_snapshot_periodic("poll_tick").await;
    assert_eq!(snapshot_count(&pool).await, base + 1);

    // t=299 < 300s Default-Intervall → kein zweiter Snapshot.
    clock_now.store(299, Ordering::SeqCst);
    manager.record_capacity_snapshot_periodic("poll_tick").await;
    assert_eq!(snapshot_count(&pool).await, base + 1);

    // t=300 >= Intervall → neuer Snapshot mit used_slots=2.
    clock_now.store(300, Ordering::SeqCst);
    manager.record_capacity_snapshot_periodic("poll_tick").await;
    assert_eq!(snapshot_count(&pool).await, base + 2);

    let (trigger, used): (String, i32) = sqlx::query_as(
        "SELECT trigger_reason, used_slots FROM twitch_eventsub_capacity_snapshot
          WHERE trigger_reason = 'poll_tick' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(trigger, "poll_tick");
    assert_eq!(used, 2);
}

#[tokio::test]
async fn periodic_capacity_snapshot_raeumt_alte_zeilen_ab() {
    let pool = pool_or_skip!("b5_08_capacity_retention");
    let transport = Arc::new(StubTransport::default());
    let (_clock_now, clock) = fake_clock();
    let manager = SubscriptionManager::new(
        transport.clone(),
        SubscriptionConfig {
            callback_url: "https://cb/x".to_string(),
            secret: "geheim".to_string(),
        },
        CapacitySnapshotStore::new(pool.clone()),
    )
    .with_clock(clock);

    // Eine Zeile weit jenseits des Default-Retention-Fensters (45 Tage).
    let stale_ts = Utc::now() - chrono::Duration::days(90);
    sqlx::query(
        "INSERT INTO twitch_eventsub_capacity_snapshot
            (ts_utc, trigger_reason, listener_count, ready_listeners, failed_listeners,
             used_slots, total_slots, headroom_slots, listeners_at_limit, utilization_pct, listeners_json)
         VALUES ($1, 'stale', 0, 0, 0, 0, 0, 0, 0, 0.0, '[]')",
    )
    .bind(stale_ts)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(snapshot_count(&pool).await, 1);

    // Erster periodischer Aufruf (t=0): schreibt frische Zeile + läuft Cleanup.
    manager.record_capacity_snapshot_periodic("poll_tick").await;

    // Stale-Zeile weg, nur die frische bleibt.
    let remaining: Vec<String> =
        sqlx::query_scalar("SELECT trigger_reason FROM twitch_eventsub_capacity_snapshot")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(remaining, vec!["poll_tick".to_string()]);
}
