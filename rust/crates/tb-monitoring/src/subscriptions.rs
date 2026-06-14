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

/// Broadcaster-Telemetrie-Subscriptions: (Typ, Version, benötigter Scope).
/// Brauchen den **Broadcaster-User-Token** (verschlüsselter `twitch_raid_auth`-
/// Store), nicht den App-Token — Port von `eventsub_mixin.py` `broadcaster_subs`
/// (Z. 1623). Werden nur angelegt, wenn der Token den jeweiligen Scope trägt.
/// Die zugehörigen Events verarbeitet der Dispatcher bereits (`store_telemetry`).
///
/// Die Moderator-Subscriptions (channel.ban/unban/shoutout/follow/moderate)
/// fehlen bewusst: sie brauchen den **Bot-Token**, der während des Strangler-
/// Cutovers noch vom Python-Chat-Prozess refresht wird — ein zweiter Refresher
/// würde die Refresh-Token-Rotation beider Prozesse gegenseitig invalidieren.
pub const BROADCASTER_TELEMETRY_SUBSCRIPTIONS: [(&str, &str, &str); 12] = [
    ("channel.cheer", "1", "bits:read"),
    ("channel.bits.use", "1", "bits:read"),
    ("channel.hype_train.begin", "1", "channel:read:hype_train"),
    ("channel.hype_train.progress", "1", "channel:read:hype_train"),
    ("channel.hype_train.end", "1", "channel:read:hype_train"),
    ("channel.subscribe", "1", "channel:read:subscriptions"),
    ("channel.subscription.gift", "1", "channel:read:subscriptions"),
    ("channel.subscription.message", "1", "channel:read:subscriptions"),
    ("channel.subscription.end", "1", "channel:read:subscriptions"),
    ("channel.ad_break.begin", "1", "channel:read:ads"),
    (
        "channel.channel_points_automatic_reward_redemption.add",
        "2",
        "channel:read:redemptions",
    ),
    (
        "channel.channel_points_custom_reward_redemption.add",
        "1",
        "channel:read:redemptions",
    ),
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
    /// Anlegen; `true` = existierte bereits (409-as-success). `bearer_override`
    /// = Some(User-/Broadcaster-Token) für Telemetrie-Subs; `None` = App-Token.
    async fn create(
        &self,
        sub_type: &str,
        version: &str,
        condition: &Value,
        callback: &str,
        secret: &str,
        bearer_override: Option<&str>,
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
    /// Permanent-Fehler (Typ, broadcaster_user_id): 403 = Bot gebannt oder
    /// Kanal sperrt externe Subs. Kein Retry bis Neustart.
    perm_failed: Mutex<HashSet<(String, String)>>,
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
            perm_failed: Mutex::new(HashSet::new()),
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

    /// Chat-Subscriptions für einen Partner-Kanal (Welle B):
    /// `channel.chat.message` + `channel.chat.notification`, Condition
    /// `{broadcaster_user_id, user_id: <bot>}`. Per Webhook + App-Token
    /// erlaubt, sobald der Bot-Account `user:bot` und der Broadcaster
    /// `channel:bot` autorisiert hat — exakt der Scope-Filter des
    /// Python-`join_partner_channels` (connection.py). „Join" = diese Subs.
    pub async fn ensure_chat_subscriptions(
        &self,
        broadcaster_id: &str,
        bot_user_id: &str,
        login: &str,
    ) -> bool {
        let mut ok = true;
        for sub_type in ["channel.chat.message", "channel.chat.notification"] {
            let condition = serde_json::json!({
                "broadcaster_user_id": broadcaster_id,
                "user_id": bot_user_id,
            });
            ok &= self
                .ensure_subscription_with_condition(
                    sub_type,
                    "1",
                    condition,
                    broadcaster_id,
                    login,
                    None,
                )
                .await;
        }
        ok
    }

    /// Broadcaster-Telemetrie-Subs (Bits/Subs/Hype/Ads/Channel-Points) für einen
    /// Partner sicherstellen — Port von `eventsub_mixin.py` Schritt 3 (Z. 1599).
    /// `token` ist der Broadcaster-User-Token, `scopes` dessen DB-Scopes; ein
    /// Sub wird nur versucht, wenn der Scope vorhanden ist (oder die Scope-Liste
    /// leer/unbekannt ist — dann alle versuchen, wie Python). Liefert die Anzahl
    /// sichergestellter Subs (getrackt oder neu erstellt).
    pub async fn ensure_broadcaster_telemetry_subscriptions(
        &self,
        broadcaster_id: &str,
        login: &str,
        token: &str,
        scopes: &[String],
    ) -> usize {
        let broadcaster_id = broadcaster_id.trim();
        if broadcaster_id.is_empty() || token.trim().is_empty() {
            return 0;
        }
        let scope_set: HashSet<String> = scopes
            .iter()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        let condition = serde_json::json!({ "broadcaster_user_id": broadcaster_id });
        let mut ensured = 0;
        for (sub_type, version, required_scope) in BROADCASTER_TELEMETRY_SUBSCRIPTIONS {
            // Scope-Filter (Python: `if token_scopes and required_scope not in token_scopes`):
            // bei bekannter Scope-Liste fehlende Scopes überspringen.
            if !scope_set.is_empty() && !scope_set.contains(required_scope) {
                tracing::debug!(
                    sub_type,
                    login,
                    required_scope,
                    "Telemetrie-Sub übersprungen: Token-Scope fehlt"
                );
                continue;
            }
            if self
                .ensure_subscription_with_condition(
                    sub_type,
                    version,
                    condition.clone(),
                    broadcaster_id,
                    login,
                    Some(token),
                )
                .await
            {
                ensured += 1;
            }
        }
        ensured
    }

    async fn ensure_subscription_with_key(
        &self,
        sub_type: &str,
        version: &str,
        condition_key: &str,
        broadcaster_id: &str,
        login: &str,
    ) -> bool {
        let condition = serde_json::json!({ condition_key: broadcaster_id.trim() });
        self.ensure_subscription_with_condition(
            sub_type,
            version,
            condition,
            broadcaster_id,
            login,
            None,
        )
        .await
    }

    async fn ensure_subscription_with_condition(
        &self,
        sub_type: &str,
        version: &str,
        condition: serde_json::Value,
        broadcaster_id: &str,
        login: &str,
        bearer_override: Option<&str>,
    ) -> bool {
        let broadcaster_id = broadcaster_id.trim();
        if broadcaster_id.is_empty() {
            return false;
        }
        if self.is_tracked(sub_type, broadcaster_id) {
            tracing::debug!(sub_type, login, "EventSub: bereits subscribed, überspringe");
            return true;
        }
        if self
            .perm_failed
            .lock()
            .expect("perm_failed lock")
            .contains(&(sub_type.to_string(), broadcaster_id.to_string()))
        {
            tracing::debug!(sub_type, login, "EventSub: 403-gebannt, überspringe");
            return false;
        }
        match self
            .transport
            .create(
                sub_type,
                version,
                &condition,
                &self.config.callback_url,
                &self.config.secret,
                bearer_override,
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
                let msg = error.to_string();
                if msg.contains("403") {
                    self.perm_failed
                        .lock()
                        .expect("perm_failed lock")
                        .insert((sub_type.to_string(), broadcaster_id.to_string()));
                    tracing::warn!(
                        sub_type,
                        login,
                        "EventSub 403: Bot gebannt oder Kanal gesperrt — \
                         kein weiterer Retry bis Neustart"
                    );
                } else if msg.contains("429") {
                    // Rate-Limit: transient, nächster Reconcile-Zyklus versucht erneut.
                    // debug! statt warn! — sonst gleicher Spam wie 403 (48 Kanäle × 30 min).
                    tracing::debug!(sub_type, login, "EventSub 429: Rate-Limit — Retry nächster Zyklus");
                } else if msg.contains("401") {
                    // App-Token abgelaufen/ungültig: TokenManager übernimmt Refresh.
                    // debug! — betrifft alle Kanäle gleichzeitig, würde sonst 48× spammen.
                    tracing::debug!(sub_type, login, "EventSub 401: App-Token temporär ungültig");
                } else if msg.contains("400") {
                    // Kanal für diesen Sub-Typ nicht berechtigt (z. B. hype_train
                    // braucht Affiliate/Partner-Tier) oder Scope-Edge-Case. Python
                    // fängt das in den broadcaster_subs still auf debug ab — nächster
                    // Reconcile-Zyklus versucht es erneut, falls sich die Lage ändert.
                    tracing::debug!(sub_type, login, "EventSub 400: Kanal nicht berechtigt — Retry nächster Zyklus");
                } else {
                    tracing::warn!(%error, sub_type, login, "EventSub: Subscription fehlgeschlagen");
                }
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

    /// Aktuell getrackte EventSub-Subscriptions als `(sub_type, broadcaster_id)`-
    /// Paare — Live-Quelle für die `current`-Sektion von `GET /stats` (Webhook-
    /// Modus; ersetzt Pythons WS-In-Process-State).
    pub fn tracked_pairs(&self) -> Vec<(String, String)> {
        self.tracked
            .lock()
            .expect("tracked lock")
            .iter()
            .cloned()
            .collect()
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
