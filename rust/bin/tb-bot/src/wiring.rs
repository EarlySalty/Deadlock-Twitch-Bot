//! Composition-Root des Monitorings: Adapter zwischen tb-monitoring-Ports
//! und den Transport-Crates (Hexagonal — die Ports kennen kein Helix).

use std::sync::Arc;

use serde_json::Value;
use tb_monitoring::poller::source::SourceError;
use tb_monitoring::{
    EventSubHooks, RemoteSubscription, SubscriptionManager, SubscriptionTransport,
};
use tb_transport_twitch::eventsub::CreateOutcome;
use tb_transport_twitch::HelixClient;

/// Helix-Adapter für den Subscription-Port (App-Token, Core-Subs).
pub struct HelixSubscriptionTransport {
    pub helix: HelixClient,
}

#[async_trait::async_trait]
impl SubscriptionTransport for HelixSubscriptionTransport {
    async fn create(
        &self,
        sub_type: &str,
        version: &str,
        condition: &Value,
        callback: &str,
        secret: &str,
    ) -> Result<bool, SourceError> {
        let outcome = self
            .helix
            .create_eventsub_webhook_subscription(
                sub_type, version, condition, callback, secret, None,
            )
            .await?;
        Ok(outcome == CreateOutcome::AlreadyExists)
    }

    async fn list(&self) -> Result<Vec<RemoteSubscription>, SourceError> {
        let subs = self.helix.list_eventsub_subscriptions(None).await?;
        Ok(subs
            .into_iter()
            .map(|sub| RemoteSubscription {
                id: sub.id,
                sub_type: sub.sub_type,
                status: sub.status,
                callback: sub.transport.callback,
                broadcaster_user_id: sub
                    .condition
                    .get("broadcaster_user_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
            .collect())
    }

    async fn delete(&self, id: &str) -> Result<(), SourceError> {
        self.helix.delete_eventsub_subscription(id).await?;
        Ok(())
    }
}

/// EventSub-Hooks mit Subscription-Wirkung: Go-Live registriert die
/// stream.offline-Subscription (Python `_handle_stream_went_live`).
/// Raid-/Score-Hooks bleiben Noop bis zur Raid-Phase (Cutover-Kopplung).
pub struct SubscriptionEventSubHooks {
    pub manager: Arc<SubscriptionManager>,
}

#[async_trait::async_trait]
impl EventSubHooks for SubscriptionEventSubHooks {
    async fn on_stream_went_live(&self, twitch_user_id: &str, login: &str) {
        self.manager
            .ensure_offline_subscription(twitch_user_id, login)
            .await;
    }
}
