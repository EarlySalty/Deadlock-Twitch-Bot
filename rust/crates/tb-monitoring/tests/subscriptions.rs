//! Tests des Subscription-Lifecycles (Slice 4d-ii): Ensure mit Tracking-Dedup,
//! Cleanup nur für eigene Callback-URL + inaktive Ziele, Capacity-Snapshot.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

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
    deletes: Mutex<Vec<String>>,
    listing: Mutex<Vec<RemoteSubscription>>,
}

#[async_trait::async_trait]
impl SubscriptionTransport for StubTransport {
    async fn create(
        &self,
        sub_type: &str,
        _version: &str,
        condition: &serde_json::Value,
        _callback: &str,
        _secret: &str,
    ) -> Result<bool, SourceError> {
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
