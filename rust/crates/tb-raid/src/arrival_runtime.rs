//! `on_raid_arrival`-Runtime — führt einen [`RaidSignalPlan`] aus, indem sie
//! jede Action an einen [`RaidArrivalSink`]-Port dispatcht. Port von
//! `raid_arrival_runtime.py` `_execute_signal_plan_actions` (Z. 168–229).
//!
//! Bewusste Trennung: Die Runtime ist ein **dünner, testbarer Dispatcher** über
//! den Plan. Die tatsächlichen Effekte (Pending-Store schreiben, Arrival +
//! Score-Tracking bei Confirm, Manual-Raid-Lock, Orphan-Notification) liegen
//! hinter dem Sink-Port — dessen echte Impl wird in der Composition-Root gegen
//! die Stores + den Confirm-Resolver (`live_state`-Reads) verdrahtet. So bleibt
//! `tb-raid` von den Monitoring-Tabellen entkoppelt.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{json, Value};
use tb_observability::RaidObservabilityService;

use crate::pending_raids::PendingRaid;
use crate::signal_correlation::{ActionData, RaidSignalPlan};

/// Effekt-Port der Arrival-Runtime. Jede Methode entspricht einem
/// Action-Kind aus dem Plan; die Composition-Root verdrahtet sie an die Stores.
#[async_trait::async_trait]
pub trait RaidArrivalSink: Send + Sync {
    /// `record_secondary_signal` — sekundäres Signal vermerken (kein neuer Raid).
    #[allow(clippy::too_many_arguments)] // Faithful zur Python-Action-Signatur.
    async fn record_secondary_signal(
        &self,
        signal_type: &str,
        from_broadcaster_login: &str,
        from_broadcaster_id: Option<&str>,
        to_broadcaster_login: &str,
        to_broadcaster_id: &str,
        viewer_count: i32,
        unraid_seen: bool,
    );

    /// `record_pending_observation` — diagnostische Beobachtung zu einem Pending.
    async fn record_pending_observation(
        &self,
        pending: &PendingRaid,
        signal_type: &str,
        status: &str,
        reason: Option<&str>,
        detail: Option<&str>,
    );

    /// `store_pending_raid` — Pending-Raid anlegen/aktualisieren.
    async fn store_pending_raid(&self, pending: &PendingRaid);

    /// `store_orphan_chat_notification` — Chat-Notification ohne Pending-Kontext.
    #[allow(clippy::too_many_arguments)]
    async fn store_orphan_chat_notification(
        &self,
        to_broadcaster_id: &str,
        to_broadcaster_login: &str,
        from_broadcaster_id: Option<&str>,
        from_broadcaster_login: &str,
        viewer_count: i32,
        message_id: Option<&str>,
        event_timestamp: Option<&str>,
    );

    /// `confirm_pending_raid` — Pending als bestätigten Arrival abschließen
    /// (Arrival-Tracking + Score-Tracking + Pending entfernen).
    async fn confirm_pending_raid(
        &self,
        signal_type: &str,
        to_broadcaster_id: &str,
        to_broadcaster_login: &str,
        from_broadcaster_login: &str,
        from_broadcaster_id: Option<&str>,
        viewer_count: i32,
    );

    /// `mark_manual_raid_started` — Manual-Raid-TTL-Lock setzen.
    async fn mark_manual_raid_started(&self, source_key: &str, ttl_seconds: f64);

    /// `record_independent_raid_arrival` — Arrival ohne Pending-Kontext.
    async fn record_independent_raid_arrival(
        &self,
        signal_type: &str,
        from_broadcaster_login: &str,
        from_broadcaster_id: Option<&str>,
        to_broadcaster_login: &str,
        to_broadcaster_id: &str,
        viewer_count: i32,
    );
}

/// Führt Korrelations-Pläne gegen einen [`RaidArrivalSink`] aus.
pub struct RaidArrivalRuntime {
    sink: Arc<dyn RaidArrivalSink>,
    observability: Option<Arc<RaidObservabilityService>>,
}

impl RaidArrivalRuntime {
    pub fn new(sink: Arc<dyn RaidArrivalSink>) -> Self {
        Self {
            sink,
            observability: None,
        }
    }

    /// Verdrahtet Raid-Observability fuer Arrival-Emitter.
    ///
    /// WIRING-TODO(P2.44): bin/tb-bot/src/main.rs soll den
    /// `RaidObservabilityService` in die Arrival-Runtime injizieren.
    #[must_use]
    pub fn with_observability(mut self, observability: Arc<RaidObservabilityService>) -> Self {
        self.observability = Some(observability);
        self
    }

    /// Führt alle Actions eines Plans der Reihe nach aus (wie Python).
    pub async fn execute_plan(&self, plan: &RaidSignalPlan) {
        for action in &plan.actions {
            self.execute_action(&action.data).await;
        }
    }

    async fn execute_action(&self, data: &ActionData) {
        match data {
            ActionData::SecondarySignal {
                signal_type,
                from_broadcaster_login,
                from_broadcaster_id,
                to_broadcaster_login,
                to_broadcaster_id,
                viewer_count,
                unraid_seen,
            } => {
                self.sink
                    .record_secondary_signal(
                        signal_type,
                        from_broadcaster_login,
                        from_broadcaster_id.as_deref(),
                        to_broadcaster_login,
                        to_broadcaster_id,
                        *viewer_count,
                        *unraid_seen,
                    )
                    .await;
            }
            ActionData::PendingObservation {
                pending_raid,
                signal_type,
                status,
                reason,
                detail,
            } => {
                self.sink
                    .record_pending_observation(
                        pending_raid,
                        signal_type,
                        status,
                        *reason,
                        detail.as_deref(),
                    )
                    .await;
            }
            ActionData::StorePendingRaid { pending_raid } => {
                self.sink.store_pending_raid(pending_raid).await;
            }
            ActionData::OrphanChatNotification {
                to_broadcaster_id,
                to_broadcaster_login,
                from_broadcaster_id,
                from_broadcaster_login,
                viewer_count,
                message_id,
                event_timestamp,
            } => {
                self.sink
                    .store_orphan_chat_notification(
                        to_broadcaster_id,
                        to_broadcaster_login,
                        from_broadcaster_id.as_deref(),
                        from_broadcaster_login,
                        *viewer_count,
                        message_id.as_deref(),
                        event_timestamp.as_deref(),
                    )
                    .await;
                self.emit_orphan_chat_observability(
                    to_broadcaster_id,
                    to_broadcaster_login,
                    from_broadcaster_id.as_deref(),
                    from_broadcaster_login,
                    *viewer_count,
                    message_id.as_deref(),
                );
            }
            ActionData::ConfirmPendingRaid {
                signal_type,
                to_broadcaster_id,
                to_broadcaster_login,
                from_broadcaster_login,
                from_broadcaster_id,
                viewer_count,
            } => {
                self.sink
                    .confirm_pending_raid(
                        signal_type,
                        to_broadcaster_id,
                        to_broadcaster_login,
                        from_broadcaster_login,
                        from_broadcaster_id.as_deref(),
                        *viewer_count,
                    )
                    .await;
            }
            ActionData::MarkManualRaidStarted {
                source_key,
                ttl_seconds,
            } => {
                // Python: nur bei nicht-leerem source_key (Z. 211–215).
                if !source_key.trim().is_empty() {
                    self.sink
                        .mark_manual_raid_started(source_key, *ttl_seconds)
                        .await;
                }
            }
            ActionData::IndependentRaidArrival {
                signal_type,
                from_broadcaster_login,
                from_broadcaster_id,
                to_broadcaster_login,
                to_broadcaster_id,
                viewer_count,
            } => {
                self.sink
                    .record_independent_raid_arrival(
                        signal_type,
                        from_broadcaster_login,
                        from_broadcaster_id.as_deref(),
                        to_broadcaster_login,
                        to_broadcaster_id,
                        *viewer_count,
                    )
                    .await;
            }
        }
    }

    fn emit_orphan_chat_observability(
        &self,
        to_broadcaster_id: &str,
        to_broadcaster_login: &str,
        from_broadcaster_id: Option<&str>,
        from_broadcaster_login: &str,
        viewer_count: i32,
        message_id: Option<&str>,
    ) {
        let Some(service) = &self.observability else {
            return;
        };
        service.increment_counter("raid_orphan_chat_notification_total", 1);
        let flow_id = service.next_flow_id("raid-orphan");
        let mut details: BTreeMap<String, Value> = BTreeMap::new();
        details.insert("viewer_count".to_string(), json!(viewer_count));
        details.insert(
            "message_id".to_string(),
            message_id.map(|id| json!(id)).unwrap_or(Value::Null),
        );
        service.emit_event(
            "raid",
            &flow_id,
            "orphan_chat",
            "stored",
            Some(from_broadcaster_login),
            from_broadcaster_id,
            Some(to_broadcaster_login),
            Some(to_broadcaster_id),
            details,
        );
    }
}
