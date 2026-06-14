//! Adapter: füllt die `current`-Sektion von `GET /stats` (EventSub) aus dem
//! nativen [`SubscriptionManager`].
//!
//! Webhook-Modell (ADR 0004): Es gibt keine WebSocket-Listener wie in Pythons
//! In-Process-State — die Listener-Felder bleiben 0 (konsistent mit
//! `record_capacity_snapshot`). Der Live-Mehrwert sind `subscription_count`,
//! `used_slots` und die Typ-/Kanal-Aufschlüsselung aus dem getrackten Set.
//! Shapes 1:1 zu Python `_collect_eventsub_capacity_snapshot`
//! (`eventsub_mixin.py:594-636`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use serde_json::json;
use sqlx::PgPool;
use tb_internal_api::{EventSubCurrentSnapshot, EventSubStatsSource};
use tb_monitoring::SubscriptionManager;

/// Live-EventSub-Stats aus dem SubscriptionManager + DB-Login-Auflösung.
pub struct ManagerEventSubStats {
    manager: Arc<SubscriptionManager>,
    pool: PgPool,
}

impl ManagerEventSubStats {
    pub fn new(manager: Arc<SubscriptionManager>, pool: PgPool) -> Self {
        Self { manager, pool }
    }
}

#[async_trait::async_trait]
impl EventSubStatsSource for ManagerEventSubStats {
    async fn get_snapshot(&self) -> Option<EventSubCurrentSnapshot> {
        // (sub_type, broadcaster_id)-Paare des Live-Sets.
        let pairs = self.manager.tracked_pairs();
        let count = pairs.len() as i64;

        // Aufschlüsselung nach Typ und nach Kanal.
        let mut type_counts: BTreeMap<String, i64> = BTreeMap::new();
        let mut channels: BTreeMap<String, (i64, BTreeSet<String>)> = BTreeMap::new();
        for (sub_type, broadcaster_id) in &pairs {
            *type_counts.entry(sub_type.clone()).or_default() += 1;
            let entry = channels.entry(broadcaster_id.clone()).or_default();
            entry.0 += 1;
            entry.1.insert(sub_type.clone());
        }

        let login_map = resolve_logins(&self.pool, channels.keys().cloned().collect()).await;

        // subscription_types: [{sub_type, count}] sortiert -count, sub_type.
        let mut types_vec: Vec<(String, i64)> = type_counts.into_iter().collect();
        types_vec.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let active_subscription_types = types_vec
            .iter()
            .map(|(sub_type, c)| json!({"sub_type": sub_type, "count": c}))
            .collect();

        // subscription_channels: sortiert -count, login, twitch_user_id.
        let mut chan_vec: Vec<(String, i64, Vec<String>, Option<String>)> = channels
            .into_iter()
            .map(|(bid, (c, types))| {
                let login = login_map.get(&bid).cloned().flatten();
                (bid, c, types.into_iter().collect(), login)
            })
            .collect();
        chan_vec.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| a.3.clone().unwrap_or_default().cmp(&b.3.clone().unwrap_or_default()))
                .then_with(|| a.0.cmp(&b.0))
        });
        let active_subscription_channels = chan_vec
            .iter()
            .map(|(bid, c, types, login)| {
                json!({
                    "twitch_user_id": bid,
                    "twitch_login": login,
                    "subscription_count": c,
                    "sub_types": types,
                })
            })
            .collect();

        // active_subscriptions: flache Liste (Python-Shape ist hier variabel).
        let active_subscriptions = pairs
            .iter()
            .map(|(sub_type, bid)| {
                json!({
                    "sub_type": sub_type,
                    "target_user_id": bid,
                    "target_login": login_map.get(bid).cloned().flatten(),
                })
            })
            .collect();

        Some(EventSubCurrentSnapshot {
            ts_utc: chrono::Utc::now(),
            // Webhook-Modus: keine WS-Listener (by design) — wie record_capacity_snapshot.
            listener_count: 0,
            ready_listeners: 0,
            failed_listeners: 0,
            used_slots: count,
            total_slots: 0,
            headroom_slots: 0,
            listeners_at_limit: 0,
            utilization_pct: 0.0,
            subscription_count: count,
            active_subscriptions,
            active_subscription_types,
            active_subscription_channels,
        })
    }
}

/// Löst `broadcaster_id → twitch_login` best-effort über `twitch_streamer_identities`.
/// Bei DB-Fehler leere Map (Logins erscheinen dann als `null`, wie Python erlaubt).
async fn resolve_logins(pool: &PgPool, ids: Vec<String>) -> HashMap<String, Option<String>> {
    if ids.is_empty() {
        return HashMap::new();
    }
    let rows: Result<Vec<(String, Option<String>)>, _> = sqlx::query_as(
        "SELECT twitch_user_id, twitch_login
           FROM twitch_streamer_identities
          WHERE twitch_user_id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await;
    match rows {
        Ok(rows) => rows
            .into_iter()
            .map(|(id, login)| {
                let login = login
                    .map(|l| l.trim().to_lowercase())
                    .filter(|l| !l.is_empty());
                (id, login)
            })
            .collect(),
        Err(error) => {
            tracing::debug!(%error, "EventSub-Stats: Login-Auflösung fehlgeschlagen");
            HashMap::new()
        }
    }
}
