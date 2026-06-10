//! EventSub-Subscription-Lifecycle (Webhook-Transport, ADR 0004).
//!
//! Rust verwaltet die **Core-Subscriptions** (stream.online/offline,
//! channel.update) mit App-Token selbst — das ist alles, was das Monitoring
//! braucht. Die Broadcaster-/Moderator-Telemetrie-Subs (Bits/Subs/Bans/…)
//! brauchen User-Tokens aus dem verschlüsselten `twitch_raid_auth`-Store
//! (Raid-Subsystem, Phase 6) und bleiben bis dahin bei Python; bei Twitch
//! bestehende Subscriptions liefern unabhängig vom Ersteller weiter an
//! dieselbe Callback-URL (Cutover-Kopplung, siehe Plan-Doc).

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;

use crate::poller::source::SourceError;

/// Core-Subscriptions des Monitorings: (Typ, Version).
pub const CORE_SUBSCRIPTIONS: [(&str, &str); 3] = [
    ("stream.online", "1"),
    ("stream.offline", "1"),
    ("channel.update", "2"),
];

/// Eine bei Twitch registrierte Subscription (Transport-neutrale Sicht).
#[derive(Debug, Clone)]
pub struct RemoteSubscription {
    pub id: String,
    pub sub_type: String,
    pub status: String,
    pub callback: Option<String>,
    pub broadcaster_user_id: Option<String>,
}

/// Port zum Subscription-Backend (Helix-Adapter lebt in `tb-bot`).
#[async_trait::async_trait]
pub trait SubscriptionTransport: Send + Sync {
    /// Anlegen; `true` = existierte bereits (409-as-success).
    async fn create(
        &self,
        sub_type: &str,
        version: &str,
        condition: &Value,
        callback: &str,
        secret: &str,
    ) -> Result<bool, SourceError>;
    async fn list(&self) -> Result<Vec<RemoteSubscription>, SourceError>;
    async fn delete(&self, id: &str) -> Result<(), SourceError>;
}

/// Webhook-Konfiguration (Callback-URL + Secret aus der Env).
#[derive(Debug, Clone)]
pub struct SubscriptionConfig {
    pub callback_url: String,
    pub secret: String,
}

pub struct SubscriptionManager {
    transport: Arc<dyn SubscriptionTransport>,
    config: SubscriptionConfig,
    capacity: CapacitySnapshotStore,
    /// In-Memory-Tracking (Typ, broadcaster_user_id) — wie Pythons
    /// `_eventsub_has_sub`, beim Start via [`Self::rehydrate`] gefüllt.
    tracked: Mutex<HashSet<(String, String)>>,
}

impl SubscriptionManager {
    pub fn new(
        transport: Arc<dyn SubscriptionTransport>,
        config: SubscriptionConfig,
        capacity: CapacitySnapshotStore,
    ) -> Self {
        Self {
            transport,
            config,
            capacity,
            tracked: Mutex::new(HashSet::new()),
        }
    }

    fn is_tracked(&self, sub_type: &str, broadcaster_id: &str) -> bool {
        self.tracked
            .lock()
            .expect("tracked lock")
            .contains(&(sub_type.to_string(), broadcaster_id.to_string()))
    }

    fn track(&self, sub_type: &str, broadcaster_id: &str) {
        self.tracked
            .lock()
            .expect("tracked lock")
            .insert((sub_type.to_string(), broadcaster_id.to_string()));
    }

    /// Tracking aus dem Twitch-Bestand aufbauen (enabled + unsere Callback).
    pub async fn rehydrate(&self) {
        match self.transport.list().await {
            Ok(subs) => {
                let mut tracked = self.tracked.lock().expect("tracked lock");
                tracked.clear();
                for sub in subs {
                    if sub.status != "enabled" {
                        continue;
                    }
                    if sub.callback.as_deref() != Some(self.config.callback_url.as_str()) {
                        continue;
                    }
                    if let Some(bid) = &sub.broadcaster_user_id {
                        tracked.insert((sub.sub_type, bid.clone()));
                    }
                }
                tracing::info!(count = tracked.len(), "EventSub-Tracking rehydriert");
            }
            Err(error) => {
                tracing::warn!(%error, "EventSub-Tracking konnte nicht rehydriert werden");
            }
        }
    }

    /// stream.offline für einen Broadcaster sicherstellen
    /// (Python `_ensure_eventsub_offline_subscription`, Webhook-Pfad).
    pub async fn ensure_offline_subscription(&self, broadcaster_id: &str, login: &str) -> bool {
        self.ensure_subscription("stream.offline", "1", broadcaster_id, login)
            .await
    }

    /// `channel.raid`-Subscription für ein Raid-Ziel: Condition ist
    /// `to_broadcaster_user_id` (Events kommen beim ZIEL an, nicht der Quelle).
    /// Python `ensure_raid_arrival_subscription_ready` (best-effort).
    pub async fn ensure_raid_subscription(&self, to_broadcaster_id: &str, login: &str) -> bool {
        self.ensure_subscription_with_key(
            "channel.raid",
            "1",
            "to_broadcaster_user_id",
            to_broadcaster_id,
            login,
        )
        .await
    }

    /// Alle Core-Subscriptions für einen Broadcaster sicherstellen.
    pub async fn ensure_core_subscriptions(&self, broadcaster_id: &str, login: &str) {
        for (sub_type, version) in CORE_SUBSCRIPTIONS {
            self.ensure_subscription(sub_type, version, broadcaster_id, login)
                .await;
        }
    }

    async fn ensure_subscription(
        &self,
        sub_type: &str,
        version: &str,
        broadcaster_id: &str,
        login: &str,
    ) -> bool {
        self.ensure_subscription_with_key(
            sub_type,
            version,
            "broadcaster_user_id",
            broadcaster_id,
            login,
        )
        .await
    }

    async fn ensure_subscription_with_key(
        &self,
        sub_type: &str,
        version: &str,
        condition_key: &str,
        broadcaster_id: &str,
        login: &str,
    ) -> bool {
        let broadcaster_id = broadcaster_id.trim();
        if broadcaster_id.is_empty() {
            return false;
        }
        if self.is_tracked(sub_type, broadcaster_id) {
            tracing::debug!(sub_type, login, "EventSub: bereits subscribed, überspringe");
            return true;
        }
        let condition = serde_json::json!({ condition_key: broadcaster_id });
        match self
            .transport
            .create(
                sub_type,
                version,
                &condition,
                &self.config.callback_url,
                &self.config.secret,
            )
            .await
        {
            Ok(already_exists) => {
                self.track(sub_type, broadcaster_id);
                tracing::info!(
                    sub_type,
                    login,
                    "EventSub Webhook: Subscription {}",
                    if already_exists {
                        "bereits vorhanden"
                    } else {
                        "erstellt"
                    }
                );
                if sub_type == "stream.offline" {
                    self.record_capacity_snapshot("stream_offline_subscribed")
                        .await;
                }
                true
            }
            Err(error) => {
                tracing::warn!(%error, sub_type, login, "EventSub: Subscription fehlgeschlagen");
                false
            }
        }
    }

    /// Räumt verwaiste Subscriptions unserer Callback-URL ab
    /// (Python `_cleanup_old_eventsub_subscriptions`): Ziel-Broadcaster
    /// nicht mehr aktiv → löschen. Liefert die Anzahl gelöschter Subs.
    pub async fn cleanup_stale(&self, active_user_ids: &HashSet<String>) -> usize {
        let subs = match self.transport.list().await {
            Ok(subs) => subs,
            Err(error) => {
                tracing::debug!(%error, "EventSub-Cleanup: Liste nicht ladbar");
                return 0;
            }
        };
        let mut deleted = 0;
        for sub in subs {
            if sub.callback.as_deref() != Some(self.config.callback_url.as_str()) {
                continue;
            }
            let target = sub.broadcaster_user_id.as_deref().unwrap_or("");
            if !active_user_ids.is_empty() && !target.is_empty() && active_user_ids.contains(target)
            {
                continue;
            }
            match self.transport.delete(&sub.id).await {
                Ok(()) => {
                    deleted += 1;
                    self.tracked
                        .lock()
                        .expect("tracked lock")
                        .remove(&(sub.sub_type, target.to_string()));
                }
                Err(error) => {
                    tracing::debug!(%error, sub_id = %sub.id, "EventSub-Cleanup: Delete fehlgeschlagen");
                }
            }
        }
        if deleted > 0 {
            tracing::info!(
                deleted,
                "EventSub Webhook: veraltete Subscriptions gelöscht"
            );
        }
        deleted
    }

    /// Kapazitäts-Snapshot fürs Admin-Dashboard. Webhook-Modus: keine
    /// WS-Listener — Listener-Felder 0, `used_slots` = getrackte Subs.
    pub async fn record_capacity_snapshot(&self, trigger: &str) {
        let used = self.tracked.lock().expect("tracked lock").len() as i32;
        if let Err(error) = self.capacity.record(trigger, used, Utc::now()).await {
            tracing::debug!(%error, trigger, "Capacity-Snapshot fehlgeschlagen");
        }
    }
}

/// Schreibt `twitch_eventsub_capacity_snapshot` (Prod-Typen verifiziert).
#[derive(Clone)]
pub struct CapacitySnapshotStore {
    pool: PgPool,
}

impl CapacitySnapshotStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn record(
        &self,
        trigger: &str,
        used_slots: i32,
        ts: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO twitch_eventsub_capacity_snapshot
                (ts_utc, trigger_reason, listener_count, ready_listeners,
                 failed_listeners, used_slots, total_slots, headroom_slots,
                 listeners_at_limit, utilization_pct, listeners_json)
            VALUES ($1, $2, 0, 0, 0, $3, 0, 0, 0, 0.0, '[]')
            "#,
        )
        .bind(ts)
        .bind(trigger)
        .bind(used_slots)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
