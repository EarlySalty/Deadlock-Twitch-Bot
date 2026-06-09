//! EventSub-Ingress des Rust-Monitorings: nimmt Bridge-Dispatches entgegen
//! (`POST /eventsub/dispatch`, Vertrag des Python-Dashboard-Service),
//! dedupliziert per Guard-Store und routet:
//!
//! - Core-Typen (`stream.online`/`stream.offline`/`channel.update`) →
//!   durable Processing-Inbox (Enqueue-Modus; Python nutzt ihn im WS-Pfad,
//!   der Webhook-Pfad lief inline — bewusste Vereinheitlichung, durable).
//! - Telemetrie-Typen (Bits/Subs/Follows/…) → direkter Insert (best-effort,
//!   Fehler werden wie in Python geloggt und verschluckt).
//! - `channel.raid` / `channel.moderate` → [`EventSubHooks`] (Raid-Subsystem,
//!   Phase 6 — Cutover-Kopplung, siehe Plan-Doc).

use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;

use crate::guard::{GuardKind, GuardStore};
use crate::inbox_runtime::{ClockFn, InboxEnqueuer};
use crate::telemetry::{HypeTrainPhase, ShoutoutDirection, TelemetryStore};

/// Message-Dedup-TTL (Python `_MAX_MESSAGE_AGE_SECONDS`).
pub const MESSAGE_DEDUP_TTL_SECONDS: f64 = 600.0;

/// Sub-Typen, die über die durable Inbox verarbeitet werden.
pub const CORE_DELIVERY_TYPES: [&str; 3] = ["stream.online", "stream.offline", "channel.update"];

/// Antwort des Dispatch-Endpoints (Bridge wertet `ok` aus).
#[derive(Debug, Clone, serde::Serialize)]
pub struct DispatchOutcome {
    pub ok: bool,
    pub duplicate: bool,
    pub queued: bool,
    pub processed: bool,
    pub sub_type: String,
}

impl DispatchOutcome {
    fn new(sub_type: &str) -> Self {
        Self {
            ok: true,
            duplicate: false,
            queued: false,
            processed: false,
            sub_type: sub_type.to_string(),
        }
    }
}

/// Hooks zu Nachbar-Subsystemen für EventSub-getriebene Effekte.
#[async_trait::async_trait]
pub trait EventSubHooks: Send + Sync {
    /// `channel.raid` angekommen (Raid-Subsystem, Phase 6).
    async fn on_channel_raid(&self, _event: &Value, _message_id: Option<&str>) {}
    /// `channel.moderate` (Blacklist-Raid-Guard, Raid-Subsystem).
    async fn on_channel_moderate(&self, _broadcaster_id: &str, _login: &str, _event: &Value) {}
    /// Go-Live-Followup: stream.offline-Subscription fürs Raid-Ziel (4d-ii).
    async fn on_stream_went_live(&self, _twitch_user_id: &str, _login: &str) {}
    /// Partner-Raid-Score-Refresh (Raid-Subsystem).
    async fn on_score_refresh(
        &self,
        _twitch_user_id: &str,
        _login: Option<&str>,
        _trigger: &'static str,
    ) {
    }
    /// stream.offline-Folgeeffekte: Auto-Raid, Global-Ban-Sweep,
    /// Engagement-Auto-Off, Post-Stream-Analyse (Cutover-Kopplungen).
    async fn on_stream_offline(&self, _twitch_user_id: &str, _login: Option<&str>) {}
}

/// Hooks ohne Wirkung (bis zur Verdrahtung in 4f).
pub struct NoopEventSubHooks;

#[async_trait::async_trait]
impl EventSubHooks for NoopEventSubHooks {}

/// Aus der Notification extrahierter Kontext
/// (Python `_extract_notification_context`).
#[derive(Debug, Clone, Default)]
pub struct NotificationContext {
    pub sub_type: String,
    pub broadcaster_id: String,
    pub broadcaster_login: String,
    pub event: Value,
}

/// Zerlegt den Bridge-Payload: `{subscription, event}` oder geschachtelt
/// unter `payload`. Raid-Events nutzen `to_broadcaster_*`.
pub fn extract_context(body: &Value, fallback_sub_type: &str) -> NotificationContext {
    let nested = body
        .get("payload")
        .filter(|p| p.get("event").is_some() || p.get("subscription").is_some());
    let scope = nested.unwrap_or(body);
    let event = scope.get("event").cloned().unwrap_or(Value::Null);
    let sub_type = scope
        .get("subscription")
        .and_then(|s| s.get("type"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or(fallback_sub_type)
        .trim()
        .to_lowercase();
    let broadcaster_id = ["broadcaster_user_id", "to_broadcaster_user_id"]
        .iter()
        .find_map(|key| event.get(*key))
        .map(json_to_trimmed_string)
        .unwrap_or_default();
    let broadcaster_login = ["broadcaster_user_login", "to_broadcaster_user_login"]
        .iter()
        .find_map(|key| event.get(*key))
        .map(json_to_trimmed_string)
        .unwrap_or_default()
        .to_lowercase();
    NotificationContext {
        sub_type,
        broadcaster_id,
        broadcaster_login,
        event,
    }
}

fn json_to_trimmed_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.trim().to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

pub struct EventSubDispatcher {
    guard: GuardStore,
    enqueuer: InboxEnqueuer,
    telemetry: TelemetryStore,
    hooks: Arc<dyn EventSubHooks>,
    clock: ClockFn,
}

impl EventSubDispatcher {
    pub fn new(
        guard: GuardStore,
        enqueuer: InboxEnqueuer,
        telemetry: TelemetryStore,
        hooks: Arc<dyn EventSubHooks>,
        clock: ClockFn,
    ) -> Self {
        Self {
            guard,
            enqueuer,
            telemetry,
            hooks,
            clock,
        }
    }

    /// Verarbeitet einen Bridge-Dispatch. `Err` = Annahme fehlgeschlagen
    /// (Bridge puffert und retryt); der Message-Guard wird dann freigegeben.
    pub async fn dispatch(
        &self,
        sub_type: &str,
        message_id: Option<&str>,
        body: &Value,
    ) -> Result<DispatchOutcome, sqlx::Error> {
        let fallback = sub_type.trim().to_lowercase();
        let context = extract_context(body, &fallback);
        let effective_type = if context.sub_type.is_empty() {
            fallback.clone()
        } else {
            context.sub_type.clone()
        };
        let message_id = message_id.map(str::trim).filter(|m| !m.is_empty());
        let now = (self.clock)();

        // Message-Dedup über Transporte/Prozesse hinweg (persistenter Guard).
        if let Some(message_id) = message_id {
            let claimed = self
                .guard
                .claim(
                    GuardKind::MessageId,
                    message_id,
                    MESSAGE_DEDUP_TTL_SECONDS,
                    now,
                )
                .await?;
            if !claimed {
                tracing::debug!(message_id, "EventSub: Duplikat-Nachricht ignoriert");
                let mut outcome = DispatchOutcome::new(&effective_type);
                outcome.duplicate = true;
                return Ok(outcome);
            }
        }
        tracing::info!(
            sub_type = %effective_type,
            broadcaster = %context.broadcaster_login,
            broadcaster_id = %context.broadcaster_id,
            message_id = message_id.unwrap_or("n/a"),
            "EventSub: Notification angenommen"
        );

        let result = self.route(&effective_type, message_id, &context).await;
        match result {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                // Annahme fehlgeschlagen → Guard freigeben, Bridge retryt.
                if let Some(message_id) = message_id {
                    let _ = self.guard.release(GuardKind::MessageId, message_id).await;
                }
                Err(error)
            }
        }
    }

    async fn route(
        &self,
        sub_type: &str,
        message_id: Option<&str>,
        context: &NotificationContext,
    ) -> Result<DispatchOutcome, sqlx::Error> {
        let mut outcome = DispatchOutcome::new(sub_type);

        if CORE_DELIVERY_TYPES.contains(&sub_type) {
            let payload = serde_json::json!({
                "broadcaster_id": context.broadcaster_id,
                "broadcaster_login": context.broadcaster_login,
                "event": context.event,
                "message_id": message_id,
            });
            self.enqueuer
                .enqueue(sub_type, &payload, message_id)
                .await?;
            outcome.queued = true;
            return Ok(outcome);
        }

        match sub_type {
            "channel.raid" => {
                self.hooks.on_channel_raid(&context.event, message_id).await;
                outcome.processed = true;
            }
            "channel.moderate" => {
                self.hooks
                    .on_channel_moderate(
                        &context.broadcaster_id,
                        &context.broadcaster_login,
                        &context.event,
                    )
                    .await;
                outcome.processed = true;
            }
            _ => {
                outcome.processed = self.store_telemetry(sub_type, context).await;
            }
        }
        Ok(outcome)
    }

    /// Telemetrie-Insert; Fehler werden (wie Pythons Inline-Callbacks)
    /// geloggt und verschluckt. `true` = Typ war bekannt.
    async fn store_telemetry(&self, sub_type: &str, context: &NotificationContext) -> bool {
        let user_id = context.broadcaster_id.as_str();
        let event = &context.event;
        let now = epoch_to_datetime((self.clock)());
        let result = match sub_type {
            "channel.cheer" | "channel.bits.use" => {
                self.telemetry.store_bits_event(user_id, event, now).await
            }
            "channel.subscribe" => {
                self.telemetry
                    .store_subscription_event(user_id, event, "subscribe", now)
                    .await
            }
            "channel.subscription.gift" => {
                self.telemetry
                    .store_subscription_event(user_id, event, "gift", now)
                    .await
            }
            "channel.subscription.message" => {
                self.telemetry
                    .store_subscription_event(user_id, event, "resub", now)
                    .await
            }
            "channel.subscription.end" => {
                self.telemetry
                    .store_subscription_event(user_id, event, "end", now)
                    .await
            }
            "channel.ad_break.begin" => {
                self.telemetry
                    .store_ad_break_event(user_id, event, now)
                    .await
            }
            "channel.hype_train.begin" => {
                self.telemetry
                    .store_hype_train_event(user_id, event, HypeTrainPhase::Begin)
                    .await
            }
            "channel.hype_train.progress" => {
                self.telemetry
                    .store_hype_train_event(user_id, event, HypeTrainPhase::Progress)
                    .await
            }
            "channel.hype_train.end" => {
                self.telemetry
                    .store_hype_train_event(user_id, event, HypeTrainPhase::End)
                    .await
            }
            "channel.ban" => {
                self.telemetry
                    .store_ban_event(user_id, event, false, now)
                    .await
            }
            "channel.unban" => {
                self.telemetry
                    .store_ban_event(user_id, event, true, now)
                    .await
            }
            "channel.shoutout.create" => {
                self.telemetry
                    .store_shoutout_event(user_id, event, ShoutoutDirection::Sent, now)
                    .await
            }
            "channel.shoutout.receive" => {
                self.telemetry
                    .store_shoutout_event(user_id, event, ShoutoutDirection::Received, now)
                    .await
            }
            "channel.follow" => {
                self.telemetry
                    .store_follow_event(user_id, &context.broadcaster_login, event, now)
                    .await
            }
            "channel.chat.user_first_message" => {
                self.telemetry
                    .store_first_message_event(user_id, &context.broadcaster_login, event, now)
                    .await
            }
            other => {
                tracing::debug!(sub_type = other, "EventSub: kein Handler für Sub-Typ");
                return false;
            }
        };
        if let Err(error) = result {
            tracing::error!(%error, sub_type, "EventSub: Telemetrie-Insert fehlgeschlagen");
        }
        true
    }
}

fn epoch_to_datetime(epoch: f64) -> DateTime<Utc> {
    Utc.timestamp_opt(epoch as i64, 0)
        .single()
        .unwrap_or_else(Utc::now)
}

#[cfg(test)]
mod tests {
    use super::extract_context;

    #[test]
    fn extract_context_flach_und_geschachtelt() {
        let flat = serde_json::json!({
            "subscription": {"type": "stream.online"},
            "event": {"broadcaster_user_id": "42", "broadcaster_user_login": "Drag"}
        });
        let ctx = extract_context(&flat, "fallback");
        assert_eq!(ctx.sub_type, "stream.online");
        assert_eq!(ctx.broadcaster_id, "42");
        assert_eq!(ctx.broadcaster_login, "drag");

        let nested = serde_json::json!({"payload": flat});
        let ctx = extract_context(&nested, "fallback");
        assert_eq!(ctx.sub_type, "stream.online");

        let raid = serde_json::json!({
            "event": {"to_broadcaster_user_id": "7", "to_broadcaster_user_login": "ziel"}
        });
        let ctx = extract_context(&raid, "channel.raid");
        assert_eq!(ctx.sub_type, "channel.raid");
        assert_eq!(ctx.broadcaster_id, "7");
        assert_eq!(ctx.broadcaster_login, "ziel");
    }
}
