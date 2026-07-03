//! Tests des Subscription-Lifecycles (Slice 4d-ii): Ensure mit Tracking-Dedup,
//! Cleanup nur für eigene Callback-URL + inaktive Ziele, Capacity-Snapshot.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use tb_monitoring::poller::source::SourceError;
use tb_monitoring::{
    BroadcasterEventSubTokenProvider, CapacitySnapshotStore, EventSubUserToken,
    ModeratorProvisionOutcome, ModeratorProvisioner, RemoteSubscription, RevocationSink,
    SubscriptionConfig, SubscriptionCreateError, SubscriptionManager, SubscriptionTransport,
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
    /// Anzahl der `create`-Aufrufe, die zuerst mit „403" scheitern (danach Ok).
    /// Steuert den P1.2-Mod-Retry-Pfad (erster 403, dann Erfolg).
    fail_403_count: AtomicU64,
    /// Sub-Typen, deren JEWEILS erster Create mit „403" scheitert (Re-Subscribe
    /// danach gelingt). Modelliert „erster Join-Versuch 403, Retry nach Re-Mod ok".
    fail_403_first_per_type: Mutex<HashSet<String>>,
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
    ) -> Result<bool, SubscriptionCreateError> {
        // Programmierter 403 (P1.2): die ersten N Creates scheitern mit 403.
        if self.fail_403_count.load(Ordering::SeqCst) > 0 {
            self.fail_403_count.fetch_sub(1, Ordering::SeqCst);
            return Err(SubscriptionCreateError::http_status(
                403,
                None,
                Some("subscription missing proper authorization".to_string()),
            ));
        }
        // Jeweils erster Create eines Sub-Typs → 403 (Retry danach gelingt).
        if self
            .fail_403_first_per_type
            .lock()
            .unwrap()
            .remove(sub_type)
        {
            return Err(SubscriptionCreateError::http_status(
                403,
                None,
                Some("subscription missing proper authorization".to_string()),
            ));
        }
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

/// Mock-Provisioner: zählt Re-Mod-Aufrufe, liefert ein konfiguriertes Ergebnis.
struct StubProvisioner {
    succeed: bool,
    outcome: Option<ModeratorProvisionOutcome>,
    calls: AtomicU64,
}
#[async_trait::async_trait]
impl ModeratorProvisioner for StubProvisioner {
    async fn ensure_bot_is_mod(&self, _broadcaster_id: &str, _login: &str) -> bool {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.succeed
    }

    async fn ensure_bot_is_mod_outcome(
        &self,
        _broadcaster_id: &str,
        _login: &str,
    ) -> ModeratorProvisionOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.outcome.unwrap_or(if self.succeed {
            ModeratorProvisionOutcome::Ready
        } else {
            ModeratorProvisionOutcome::RetryLater
        })
    }
}

/// P2.56-Stub: liefert einen festen Broadcaster-Token mit konfigurierten Scopes
/// und zählt die Auflösungen (prüft, dass der Fallback nur einmal abgefragt
/// wird, nicht pro Sub-Typ).
struct StubBroadcasterTokenProvider {
    token: String,
    scopes: Vec<String>,
    calls: AtomicU64,
}
#[async_trait::async_trait]
impl BroadcasterEventSubTokenProvider for StubBroadcasterTokenProvider {
    async fn eventsub_broadcaster_token(
        &self,
        _broadcaster_id: &str,
        _login: &str,
    ) -> Option<EventSubUserToken> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Some(EventSubUserToken::new(
            self.token.clone(),
            self.scopes.clone(),
        ))
    }
}

#[tokio::test]
async fn chat_403_mod_retry_erfolg_trackt_statt_perm_failed() {
    // P1.2: Erster 403 beim Chat-Join → Bot re-modden, Re-Subscribe gelingt →
    // Kanal getrackt, NICHT permanent perm_failed.
    let pool = pool_or_skip!("t_chat_403_retry_ok");
    let transport = Arc::new(StubTransport::default());
    // Beide Chat-Subs scheitern beim ERSTEN Versuch mit 403, der Re-Subscribe
    // nach Re-Mod gelingt dann.
    {
        let mut f = transport.fail_403_first_per_type.lock().unwrap();
        f.insert("channel.chat.message".to_string());
        f.insert("channel.chat.notification".to_string());
    }
    let provisioner = Arc::new(StubProvisioner {
        succeed: true,
        outcome: None,
        calls: AtomicU64::new(0),
    });
    let manager = SubscriptionManager::new(
        transport.clone(),
        SubscriptionConfig {
            callback_url: "https://cb/x".into(),
            secret: "s".into(),
        },
        CapacitySnapshotStore::new(pool.clone()),
    )
    .with_moderator_provisioner(provisioner.clone());

    let ok = manager
        .ensure_chat_subscriptions("555", "bot1", "partner")
        .await;
    assert!(ok, "Re-Subscribe nach Re-Mod erfolgreich → join ok");
    // Re-Mod wurde je Sub-Typ einmal versucht (2 Chat-Subs).
    assert_eq!(provisioner.calls.load(Ordering::SeqCst), 2);
    // Kanal ist NICHT permanent blockiert.
    assert!(
        !manager.chat_subscriptions_permanently_blocked("555"),
        "kein perm_failed nach erfolgreichem Mod-Retry"
    );
    // Beide Chat-Subs getrackt.
    let tracked: Vec<String> = manager
        .tracked_pairs()
        .into_iter()
        .filter(|(_, bid)| bid == "555")
        .map(|(t, _)| t)
        .collect();
    assert!(tracked.contains(&"channel.chat.message".to_string()));
    assert!(tracked.contains(&"channel.chat.notification".to_string()));
}

#[tokio::test]
async fn chat_403_mod_retry_fehlschlag_setzt_cooldown_statt_perm_failed() {
    // P1.2: Re-Mod scheitert → 10-Min-Cooldown (clearbar) STATT permanentem
    // perm_failed; nach Ablauf des Cooldowns wird erneut versucht.
    let pool = pool_or_skip!("t_chat_403_retry_cooldown");
    let transport = Arc::new(StubTransport::default());
    transport.fail_403_count.store(2, Ordering::SeqCst);
    let provisioner = Arc::new(StubProvisioner {
        succeed: false,
        outcome: None,
        calls: AtomicU64::new(0),
    });

    // Steuerbare Uhr: erlaubt das Testen des Cooldown-Ablaufs.
    let now = Arc::new(AtomicU64::new(1_000));
    let now_clk = now.clone();
    let manager = SubscriptionManager::new(
        transport.clone(),
        SubscriptionConfig {
            callback_url: "https://cb/x".into(),
            secret: "s".into(),
        },
        CapacitySnapshotStore::new(pool.clone()),
    )
    .with_moderator_provisioner(provisioner.clone())
    .with_clock(Arc::new(move || now_clk.load(Ordering::SeqCst) as f64));

    let ok = manager
        .ensure_chat_subscriptions("777", "bot1", "partner")
        .await;
    assert!(!ok, "Re-Mod fehlgeschlagen → join scheitert (vorerst)");
    // NICHT permanent blockiert — der entscheidende Unterschied zu vorher.
    assert!(
        !manager.chat_subscriptions_permanently_blocked("777"),
        "403 im Chat-Pfad darf NICHT permanent perm_failed setzen"
    );

    // Innerhalb des Cooldowns: kein neuer Create-Versuch (Gate greift).
    transport.fail_403_count.store(0, Ordering::SeqCst); // ab jetzt würde Create gelingen
    let creates_before = transport.creates.lock().unwrap().len();
    let ok_during = manager
        .ensure_chat_subscriptions("777", "bot1", "partner")
        .await;
    assert!(!ok_during, "während Cooldown übersprungen");
    assert_eq!(
        transport.creates.lock().unwrap().len(),
        creates_before,
        "kein Create-Versuch während Cooldown"
    );

    // Uhr über den 10-Min-Cooldown hinaus → automatischer Retry, jetzt Erfolg.
    now.store(1_000 + 601, Ordering::SeqCst);
    let ok_after = manager
        .ensure_chat_subscriptions("777", "bot1", "partner")
        .await;
    assert!(
        ok_after,
        "nach Cooldown-Ablauf automatischer Retry erfolgreich"
    );
    assert!(manager
        .tracked_pairs()
        .iter()
        .any(|(t, bid)| t == "channel.chat.message" && bid == "777"));
}

#[tokio::test]
async fn chat_403_bot_banned_geht_in_permission_cooldown() {
    let pool = pool_or_skip!("t_chat_403_bot_banned");
    let transport = Arc::new(StubTransport::default());
    transport.fail_403_count.store(1, Ordering::SeqCst);
    let provisioner = Arc::new(StubProvisioner {
        succeed: false,
        outcome: Some(ModeratorProvisionOutcome::BotBanned),
        calls: AtomicU64::new(0),
    });
    let manager = SubscriptionManager::new(
        transport.clone(),
        SubscriptionConfig {
            callback_url: "https://cb/x".into(),
            secret: "s".into(),
        },
        CapacitySnapshotStore::new(pool),
    )
    .with_moderator_provisioner(provisioner.clone());

    assert!(
        !manager
            .ensure_first_message_subscription("888", "bot1", "BOT_TOKEN", "banned")
            .await
    );
    assert_eq!(provisioner.calls.load(Ordering::SeqCst), 1);

    // Der zweite Lauf würde ohne Permission-Cooldown sofort wieder remodden.
    // BotBanned nutzt aber den längeren 403-Cooldown statt des 10-Min-Remod-Loops.
    assert!(
        !manager
            .ensure_first_message_subscription("888", "bot1", "BOT_TOKEN", "banned")
            .await
    );
    assert_eq!(
        provisioner.calls.load(Ordering::SeqCst),
        1,
        "Bot-Ban darf keinen sofortigen zweiten Re-Mod-Versuch auslösen"
    );
}

#[tokio::test]
async fn revocation_untrackt_und_loest_resubscribe_aus() {
    // P1.17/P1.18/P1.20: Wird eine getrackte Core-Sub von Twitch widerrufen,
    // muss sie aus `tracked` entfernt werden, damit der nächste Reconcile-Zyklus
    // (ensure_core_subscriptions) sie NEU anlegt — statt wegen is_tracked-Skip
    // dauerhaft tot zu bleiben.
    let pool = pool_or_skip!("t_subs_revocation");
    let transport = Arc::new(StubTransport::default());
    let manager = SubscriptionManager::new(
        transport.clone(),
        SubscriptionConfig {
            callback_url: "https://cb/x".to_string(),
            secret: "geheim".to_string(),
        },
        CapacitySnapshotStore::new(pool.clone()),
    );

    // Core-Subs für Broadcaster 99 anlegen → 3 Creates, danach getrackt.
    manager.ensure_core_subscriptions("99", "drag").await;
    let first = transport
        .creates
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, bid)| bid == "99")
        .count();
    assert_eq!(first, 3, "drei Core-Subs initial angelegt");

    // Re-Ensure ohne Revocation → kein neuer Create (is_tracked-Dedup greift).
    manager.ensure_core_subscriptions("99", "drag").await;
    let after_dedup = transport
        .creates
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, bid)| bid == "99")
        .count();
    assert_eq!(after_dedup, 3, "ohne Revocation kein Resubscribe");

    // Revocation für stream.online → untrack (via RevocationSink-Trait).
    let removed = RevocationSink::on_revocation(&manager, "stream.online", "99");
    assert!(removed, "stream.online war getrackt → untrack entfernt es");
    // Unbekannte Sub → nichts zu entfernen.
    assert!(!RevocationSink::on_revocation(
        &manager,
        "channel.raid",
        "99"
    ));

    // Nächster Reconcile legt NUR die widerrufene Sub neu an (die anderen zwei
    // bleiben getrackt) → genau ein zusätzlicher stream.online-Create.
    manager.ensure_core_subscriptions("99", "drag").await;
    let online_creates = transport
        .creates
        .lock()
        .unwrap()
        .iter()
        .filter(|(t, bid)| t == "stream.online" && bid == "99")
        .count();
    assert_eq!(
        online_creates, 2,
        "stream.online nach Revocation neu angelegt (Resubscribe)"
    );
    let total_99 = transport
        .creates
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, bid)| bid == "99")
        .count();
    assert_eq!(total_99, 4, "nur die widerrufene Sub wurde neu erstellt");
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
async fn rehydrate_nur_current_callback_cleanup_per_app_token_listing() {
    let pool = pool_or_skip!("t4d_subs_cleanup");
    let transport = Arc::new(StubTransport::default());
    *transport.listing.lock().unwrap() = vec![
        sub("a", "stream.offline", "https://cb/x", "42"), // aktiv → bleibt
        sub("b", "stream.offline", "https://cb/x", "99"), // inaktiv → weg
        sub("c", "stream.offline", "https://anderes/cb", "99"), // alte App-URL → weg
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
    assert_eq!(manager.cleanup_stale(&active).await, 2);
    assert_eq!(
        *transport.deletes.lock().unwrap(),
        vec!["b".to_string(), "c".to_string()]
    );
}

#[tokio::test]
async fn cleanup_stale_fail_open_bei_leerem_active_set() {
    let pool = pool_or_skip!("t4d_subs_cleanup_empty_guard");
    let transport = Arc::new(StubTransport::default());
    *transport.listing.lock().unwrap() = vec![
        sub("a", "stream.offline", "https://cb/x", "42"),
        sub("b", "channel.chat.message", "https://cb/x", "99"),
    ];
    let manager = SubscriptionManager::new(
        transport.clone(),
        SubscriptionConfig {
            callback_url: "https://cb/x".to_string(),
            secret: "geheim".to_string(),
        },
        CapacitySnapshotStore::new(pool.clone()),
    );

    let active = HashSet::new();
    assert_eq!(manager.cleanup_stale(&active).await, 0);
    assert!(transport.deletes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn cleanup_stale_loescht_alte_callback_url_trotz_aktivem_target() {
    let pool = pool_or_skip!("t4d_subs_cleanup_old_callback");
    let transport = Arc::new(StubTransport::default());
    *transport.listing.lock().unwrap() = vec![
        sub("old", "stream.offline", "https://cb/alt", "42"),
        sub("current", "stream.offline", "https://cb/x", "42"),
    ];
    let manager = SubscriptionManager::new(
        transport.clone(),
        SubscriptionConfig {
            callback_url: "https://cb/x".to_string(),
            secret: "geheim".to_string(),
        },
        CapacitySnapshotStore::new(pool.clone()),
    );

    let active: HashSet<String> = ["42".to_string()].into_iter().collect();
    assert_eq!(manager.cleanup_stale(&active).await, 1);
    assert_eq!(*transport.deletes.lock().unwrap(), vec!["old".to_string()]);
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
    assert!(!creates
        .iter()
        .any(|(t, _)| t.starts_with("channel.hype_train")));

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
    assert_eq!(
        bearers,
        vec![(
            "channel.chat.user_first_message".to_string(),
            Some("BOTTOKEN".to_string())
        )]
    );
    drop(bearers);

    // Zweiter Aufruf: getrackt → kein neuer Create.
    assert!(
        manager
            .ensure_first_message_subscription("555", "BOTID", "BOTTOKEN", "partner")
            .await
    );
    assert_eq!(transport.creates.lock().unwrap().len(), 1);

    // Leere ID / leerer Bot-Token / leere Bot-ID → kein Create.
    assert!(
        !manager
            .ensure_first_message_subscription(" ", "BOTID", "BOTTOKEN", "p")
            .await
    );
    assert!(
        !manager
            .ensure_first_message_subscription("555", "BOTID", "  ", "p")
            .await
    );
    assert!(
        !manager
            .ensure_first_message_subscription("555", " ", "BOTTOKEN", "p")
            .await
    );
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
    assert_eq!(
        types,
        vec!["channel.ban", "channel.follow", "channel.unban"]
    );
    // Shoutout mangels Scope nicht versucht.
    assert!(!creates
        .iter()
        .any(|(t, _)| t.starts_with("channel.shoutout")));
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
    assert!(bearers
        .iter()
        .all(|(_, b)| b.as_deref() == Some("BOTTOKEN")));
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

/// P2.56-Wiring: Bot-Token deckt nur Shoutout-Scope, der injizierte
/// Broadcaster-Provider deckt ban/unban/follow. Verifiziert, dass die
/// scope-fehlenden Subs über den Broadcaster-Fallback laufen
/// (`moderator_user_id = broadcaster_id`, Broadcaster-Bearer), die
/// scope-gedeckten über den Bot-Token (`moderator_user_id = bot`),
/// und dass es KEINEN Doppel-Send gibt (jeder Sub-Typ genau ein Create).
#[tokio::test]
async fn moderator_telemetry_broadcaster_fallback_fuellt_scope_luecke_ohne_doppel_send() {
    let pool = pool_or_skip!("p256_mod_telemetry_broadcaster_fallback");
    let transport = Arc::new(StubTransport::default());
    let broadcaster = Arc::new(StubBroadcasterTokenProvider {
        token: "BROKTOKEN".to_string(),
        // Broadcaster-Token deckt ban/unban/follow, NICHT shoutout.
        scopes: vec![
            "moderator:manage:banned_users".to_string(),
            "moderator:read:followers".to_string(),
        ],
        calls: AtomicU64::new(0),
    });
    let manager = SubscriptionManager::new(
        transport.clone(),
        SubscriptionConfig {
            callback_url: "https://cb/x".to_string(),
            secret: "geheim".to_string(),
        },
        CapacitySnapshotStore::new(pool.clone()),
    )
    .with_broadcaster_eventsub_token_provider(broadcaster.clone());

    // Bot-Token deckt NUR Shoutout — ban/unban/follow fehlt der Scope.
    let bot_scopes = vec!["moderator:manage:shoutouts".to_string()];
    let ensured = manager
        .ensure_moderator_telemetry_subscriptions(
            "555",
            "BOTID",
            "BOTTOKEN",
            &bot_scopes,
            "partner",
        )
        .await;
    // Alle 5 Subs sichergestellt: 2 via Bot, 3 via Broadcaster-Fallback.
    assert_eq!(ensured, 5);

    let creates = transport.creates.lock().unwrap().clone();
    let mut types: Vec<&str> = creates.iter().map(|(t, _)| t.as_str()).collect();
    types.sort_unstable();
    assert_eq!(
        types,
        vec![
            "channel.ban",
            "channel.follow",
            "channel.shoutout.create",
            "channel.shoutout.receive",
            "channel.unban",
        ],
        "jeder Sub-Typ exakt ein Create — kein Doppel-Send"
    );
    drop(creates);

    // Broadcaster-Provider EINMAL abgefragt (pro Kanal, nicht pro Sub-Typ).
    assert_eq!(broadcaster.calls.load(Ordering::SeqCst), 1);

    // Condition + Bearer pro Sub-Typ prüfen: Shoutout via Bot, Rest via
    // Broadcaster mit moderator_user_id = broadcaster_id.
    let conditions = transport.conditions.lock().unwrap().clone();
    let bearers = transport.bearers.lock().unwrap().clone();
    let bot_cond =
        serde_json::json!({ "broadcaster_user_id": "555", "moderator_user_id": "BOTID" });
    let brc_cond = serde_json::json!({ "broadcaster_user_id": "555", "moderator_user_id": "555" });
    for (sub_type, condition) in &conditions {
        let bearer = bearers
            .iter()
            .find(|(t, _)| t == sub_type)
            .and_then(|(_, b)| b.as_deref());
        if sub_type.starts_with("channel.shoutout") {
            assert_eq!(condition, &bot_cond, "{sub_type} muss Bot-Condition tragen");
            assert_eq!(
                bearer,
                Some("BOTTOKEN"),
                "{sub_type} muss Bot-Bearer nutzen"
            );
        } else {
            assert_eq!(
                condition, &brc_cond,
                "{sub_type} muss Broadcaster-Condition tragen"
            );
            assert_eq!(
                bearer,
                Some("BROKTOKEN"),
                "{sub_type} muss Broadcaster-Bearer nutzen"
            );
        }
    }
    drop(conditions);
    drop(bearers);

    // Zweiter Aufruf: alles getrackt → kein neuer Create, Provider nicht erneut
    // pro Sub-Typ befragt (is_tracked-Dedup greift im auth-attempts-Pfad).
    let again = manager
        .ensure_moderator_telemetry_subscriptions(
            "555",
            "BOTID",
            "BOTTOKEN",
            &bot_scopes,
            "partner",
        )
        .await;
    assert_eq!(again, 5);
    assert_eq!(transport.creates.lock().unwrap().len(), 5);
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
        "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) \
         VALUES ('lurker', '900')",
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
    assert_eq!(
        keys,
        vec!["channel.chat.message", "channel.chat.notification"]
    );
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

    // Streamer mit aktivem Partner → kein Lurker.
    sqlx::query(
        "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) \
         VALUES ('partner', '901')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status) \
         VALUES ('901', 'partner', 'active')",
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
    assert_eq!(
        types,
        vec!["channel.chat.message", "channel.chat.notification"]
    );
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
        "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) \
         VALUES ('raider', '902')",
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
