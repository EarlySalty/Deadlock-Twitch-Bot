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

use std::sync::atomic::{AtomicBool, Ordering};
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

/// EventSub-Typen mit registriertem Handler — das Rust-Strukturäquivalent zu
/// Pythons `set_callback`-Registry (`_has_callback`). Genau diese Typen routet
/// [`EventSubDispatcher::route`] nicht in den „unbekannt"-Zweig; jeder andere
/// Typ würde still verworfen. Quelle: Pythons `bridged_eventsub_types`
/// (dashboard_service/app.py) + die Webhook-`set_callback`-Registrierungen
/// (eventsub_core_callbacks/eventsub_mixin). `channel.moderate` ist ergänzt,
/// weil die Rust-`route` ihn behandelt (Raid-Blacklist-Guard) — Python hat ihn
/// via `set_callback` ebenfalls registriert.
pub const REGISTERED_SUB_TYPES: [&str; 25] = [
    // Core (Inbox)
    "stream.online",
    "stream.offline",
    "channel.update",
    // Hook-geroutet
    "channel.raid",
    "channel.moderate",
    "channel.chat.message",
    "channel.chat.notification",
    // Telemetrie
    "channel.follow",
    "channel.subscribe",
    "channel.subscription.gift",
    "channel.subscription.message",
    "channel.subscription.end",
    "channel.ad_break.begin",
    "channel.cheer",
    "channel.bits.use",
    "channel.hype_train.begin",
    "channel.hype_train.progress",
    "channel.hype_train.end",
    "channel.ban",
    "channel.unban",
    "channel.shoutout.create",
    "channel.shoutout.receive",
    "channel.chat.user_first_message",
    "channel.channel_points_automatic_reward_redemption.add",
    "channel.channel_points_custom_reward_redemption.add",
];

/// `true`, wenn für `sub_type` ein Handler existiert (normalisiert: getrimmt +
/// kleingeschrieben, wie [`extract_context`]). Strukturäquivalent zu Pythons
/// `_has_callback` — der Pre-Dispatch-Readiness-Check (65.3) lehnt Typen ohne
/// Handler ab, statt sie still in den „unbekannt"-Zweig laufen zu lassen.
pub fn has_registered_handler(sub_type: &str) -> bool {
    let normalized = sub_type.trim().to_lowercase();
    !normalized.is_empty() && REGISTERED_SUB_TYPES.contains(&normalized.as_str())
}

/// Pre-Dispatch-Readiness-Gate (Python `_assert_dispatch_ready`): warum eine
/// Notification VOR dem Dispatch abgelehnt wurde. Beide Varianten führen in
/// Python wie hier zu HTTP 503 (Twitch retryt).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DispatchNotReady {
    /// Notification-Dispatch ist (noch/aktuell) deaktiviert — Python
    /// `_notification_dispatch_active == False`.
    #[error("eventsub notification dispatch inactive")]
    DispatchInactive,
    /// Kein Handler für diesen Sub-Typ registriert — Python
    /// `EventSubCallbackNotRegistered`.
    #[error("eventsub callback not registered: {0}")]
    CallbackNotRegistered(String),
}

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

/// Demux-Klassen einer `channel.chat.notification` nach `notice_type`
/// (Foundation B8-00). Sub-Klassen speisen die Sub-Telemetrie (B8-01),
/// Raid/Unraid die Raid-Korrelation (B7). Port von
/// `bot/chat/bot.py::event_chat_notification` +
/// `_build_subscription_event_from_chat_notification`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatNotificationKind {
    /// `notice_type=sub` → channel.subscribe-Äquivalent.
    Sub,
    /// `notice_type=resub` → channel.subscription.message-Äquivalent.
    Resub,
    /// `notice_type=sub_gift` → channel.subscription.gift (Einzel-Geschenk).
    SubGift,
    /// `notice_type=community_sub_gift` → channel.subscription.gift (Batch).
    CommunitySubGift,
    /// `notice_type=raid` → Raid-Arrival (Ziel-seitig, B7-01).
    Raid,
    /// `notice_type=unraid` → Raid-Withdraw (B7-02 / Source-Self-Unraid B7-03).
    Unraid,
}

impl ChatNotificationKind {
    /// `true` für Sub/Resub/Gift-Klassen (Sub-Telemetrie, B8-01).
    pub fn is_subscription(self) -> bool {
        matches!(
            self,
            Self::Sub | Self::Resub | Self::SubGift | Self::CommunitySubGift
        )
    }

    /// `true` für Raid/Unraid-Klassen (Raid-Korrelation, B7).
    pub fn is_raid(self) -> bool {
        matches!(self, Self::Raid | Self::Unraid)
    }
}

/// Klassifiziert einen `channel.chat.notification`-`notice_type`. Der
/// `shared_chat_`-Präfix wird wie in Python entfernt (Shared-Chat-Sessions
/// spiegeln dieselben Notices). Unbekannte Typen → `None` (sauberes Ignorieren,
/// kein Panic). Port von `_subscription_notice_eventsub_type` (raid/bot.py) +
/// dem raid/unraid-Zweig in `event_chat_notification` (chat/bot.py).
pub fn classify_chat_notification(notice_type: &str) -> Option<ChatNotificationKind> {
    let normalized = notice_type.trim().to_lowercase();
    let normalized = normalized
        .strip_prefix("shared_chat_")
        .unwrap_or(&normalized);
    match normalized {
        "sub" => Some(ChatNotificationKind::Sub),
        "resub" => Some(ChatNotificationKind::Resub),
        "sub_gift" => Some(ChatNotificationKind::SubGift),
        "community_sub_gift" => Some(ChatNotificationKind::CommunitySubGift),
        "raid" => Some(ChatNotificationKind::Raid),
        "unraid" => Some(ChatNotificationKind::Unraid),
        _ => None,
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
    /// stream.offline-Vor-Throttle-Effekt: **Engagement-Auto-Off**. Python führt
    /// `auto_disable_on_offline` VOR dem 120s-Throttle aus (`eventsub_mixin.py`
    /// :1861) — der Engagement-Layer muss auch bei einem als Duplikat
    /// gedrosselten Offline ans Stream-Leben gekoppelt bleiben. Default no-op.
    async fn on_stream_offline_engagement(&self, _twitch_user_id: &str, _login: Option<&str>) {}

    /// stream.offline-Nach-Throttle-Effekt: **Global-Ban-Sweep** planen. Python
    /// ruft `schedule_global_ban_sweep` NACH bestandenem Throttle, aber VOR der
    /// State-Finalisierung (`eventsub_mixin.py`:1901). Default no-op.
    async fn on_stream_offline_global_ban(&self, _twitch_user_id: &str, _login: Option<&str>) {}

    /// stream.offline-Folgeeffekte nach State-Finalize: Auto-Raid + Partner-
    /// Score-Refresh + Post-Stream-Analyse (Python `eventsub_mixin.py`:1953+).
    /// Engagement-Off und Global-Ban-Sweep laufen früher (siehe oben).
    async fn on_stream_offline(&self, _twitch_user_id: &str, _login: Option<&str>) {}
    /// `channel.chat.message` (Welle B: nativer Chat-Bot — Moderation,
    /// Commands, Promos). Default no-op bis zur Chat-Verdrahtung.
    async fn on_chat_message(&self, _event: &Value, _message_id: Option<&str>) {}

    /// Routing-Punkt B8-00: `channel.chat.notification` mit Sub/Resub/Gift-
    /// `notice_type` (Sub-Telemetrie-Fallback, B8-01). `kind` ist die
    /// klassifizierte Sub-Klasse, `event` der rohe Notification-Event-Body.
    /// Default no-op bis B8-01 die Telemetrie-Persistenz verdrahtet.
    async fn on_chat_subscription_notification(
        &self,
        _kind: ChatNotificationKind,
        _event: &Value,
        _message_id: Option<&str>,
    ) {
    }

    /// Routing-Punkt B8-00: `channel.chat.notification` mit `notice_type=raid`
    /// (Raid-Arrival am Ziel, B7-01). Default no-op bis B7 die Korrelation
    /// verdrahtet.
    async fn on_chat_raid_notification(&self, _event: &Value, _message_id: Option<&str>) {}

    /// Routing-Punkt B8-00: `channel.chat.notification` mit `notice_type=unraid`
    /// (Raid-Withdraw / Source-Self-Unraid, B7-02/B7-03). Default no-op bis B7.
    async fn on_chat_unraid_notification(&self, _event: &Value, _message_id: Option<&str>) {}
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
    /// Notification-Dispatch-Schalter (Python `_notification_dispatch_active`).
    /// Anders als Python (Default `False`, an im Bridge-Setup) startet der
    /// native Receiver aktiv: er hat keinen separaten Bridge-Aktivierungs-
    /// Schritt — der lauffähige An-Zustand ist „aktiv". Der Aus-Pfad bleibt für
    /// geordnetes Pausieren/Herunterfahren erhalten.
    dispatch_active: AtomicBool,
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
            dispatch_active: AtomicBool::new(true),
        }
    }

    /// Schaltet die Notification-Annahme um (Python
    /// `activate_/deactivate_notification_dispatch`). `false` → das
    /// Readiness-Gate lehnt anschließend alle Notifications mit 503 ab.
    pub fn set_dispatch_active(&self, active: bool) {
        self.dispatch_active.store(active, Ordering::Release);
    }

    /// `true`, solange Notifications angenommen werden.
    pub fn is_dispatch_active(&self) -> bool {
        self.dispatch_active.load(Ordering::Acquire)
    }

    /// Pre-Dispatch-Readiness-Gate (Python `_assert_dispatch_ready`, 65.3):
    /// prüft VOR jedem Dispatch (a) ob die Annahme aktiv ist und (b) ob für den
    /// Sub-Typ ein Handler registriert ist. Schlägt das fehl, lehnt der Receiver
    /// mit HTTP 503 ab (Twitch retryt) — statt die Notification still in den
    /// „unbekannt"-Zweig laufen zu lassen.
    pub fn ensure_dispatch_ready(&self, sub_type: &str) -> Result<(), DispatchNotReady> {
        if !self.is_dispatch_active() {
            return Err(DispatchNotReady::DispatchInactive);
        }
        if !has_registered_handler(sub_type) {
            return Err(DispatchNotReady::CallbackNotRegistered(
                sub_type.trim().to_string(),
            ));
        }
        Ok(())
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
            "channel.chat.message" => {
                self.hooks.on_chat_message(&context.event, message_id).await;
                outcome.processed = true;
            }
            "channel.chat.notification" => {
                outcome.processed = self
                    .route_chat_notification(message_id, &context.event)
                    .await;
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

    /// Demux einer `channel.chat.notification` nach `notice_type` (B8-00).
    /// Liest `notice_type` aus dem Event-Body, klassifiziert und routet an den
    /// passenden Hook: Sub/Resub/Gift → Sub-Telemetrie (B8-01), Raid/Unraid →
    /// Raid-Korrelation (B7). Unbekannter/fehlender `notice_type` wird sauber
    /// ignoriert (kein Panic). `true` = bekannte Klasse geroutet.
    ///
    /// Foundation-Hinweis: Die Hook-Ziele sind bis B8-01/B7 Default-No-ops —
    /// dieser Zweig baut nur den Demux + die Routing-Punkte, nicht die volle
    /// Telemetrie-/Korrelations-Persistenz.
    async fn route_chat_notification(&self, message_id: Option<&str>, event: &Value) -> bool {
        let notice_type = event
            .get("notice_type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(kind) = classify_chat_notification(notice_type) else {
            tracing::debug!(
                notice_type,
                "channel.chat.notification: unbekannter notice_type ignoriert"
            );
            return false;
        };
        match kind {
            ChatNotificationKind::Raid => {
                self.hooks.on_chat_raid_notification(event, message_id).await;
            }
            ChatNotificationKind::Unraid => {
                self.hooks
                    .on_chat_unraid_notification(event, message_id)
                    .await;
            }
            sub_kind => {
                self.hooks
                    .on_chat_subscription_notification(sub_kind, event, message_id)
                    .await;
            }
        }
        true
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
            // Channel-Points-Redemptions (Python eventsub_mixin.py:2477-2493): die
            // Insert-Funktion existierte, wurde aber nie verdrahtet → Telemetrie ging
            // still verloren, weil der native Receiver die geteilte Callback-URL bedient.
            "channel.channel_points_automatic_reward_redemption.add"
            | "channel.channel_points_custom_reward_redemption.add" => {
                self.telemetry
                    .store_channel_points_event(user_id, event, now)
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
    use super::{
        classify_chat_notification, extract_context, has_registered_handler, ChatNotificationKind,
        REGISTERED_SUB_TYPES,
    };

    #[test]
    fn has_registered_handler_kennt_alle_gerouteten_typen() {
        // Jeder registrierte Typ wird erkannt (Strukturäquivalent _has_callback).
        for sub_type in REGISTERED_SUB_TYPES {
            assert!(has_registered_handler(sub_type), "{sub_type} fehlt im Gate");
        }
        // Normalisierung: Trim + Lowercase (wie extract_context).
        assert!(has_registered_handler("  Stream.Online  "));
        assert!(has_registered_handler("CHANNEL.RAID"));
        // Unbekannt / leer → kein Handler.
        assert!(!has_registered_handler("channel.unbekannt"));
        assert!(!has_registered_handler(""));
        assert!(!has_registered_handler("   "));
    }

    #[test]
    fn registry_ist_duplikatfrei() {
        // Eine doppelte Listung würde das Gate nicht brechen, wäre aber ein
        // Pflege-Smell — hier eingelockt.
        let mut sorted = REGISTERED_SUB_TYPES.to_vec();
        let len = sorted.len();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), len, "REGISTERED_SUB_TYPES enthält Duplikate");
    }

    #[test]
    fn classify_chat_notification_demuxt_nach_notice_type() {
        // Sub/Resub/Gift-Klassen → Sub-Telemetrie (B8-01).
        assert_eq!(
            classify_chat_notification("sub"),
            Some(ChatNotificationKind::Sub)
        );
        assert_eq!(
            classify_chat_notification("resub"),
            Some(ChatNotificationKind::Resub)
        );
        assert_eq!(
            classify_chat_notification("sub_gift"),
            Some(ChatNotificationKind::SubGift)
        );
        assert_eq!(
            classify_chat_notification("community_sub_gift"),
            Some(ChatNotificationKind::CommunitySubGift)
        );
        // Raid/Unraid → Raid-Korrelation (B7).
        assert_eq!(
            classify_chat_notification("raid"),
            Some(ChatNotificationKind::Raid)
        );
        assert_eq!(
            classify_chat_notification("unraid"),
            Some(ChatNotificationKind::Unraid)
        );

        // Klassen-Gruppierung (Routing-Weichen).
        assert!(ChatNotificationKind::Sub.is_subscription());
        assert!(!ChatNotificationKind::Sub.is_raid());
        assert!(ChatNotificationKind::Raid.is_raid());
        assert!(!ChatNotificationKind::Raid.is_subscription());
    }

    #[test]
    fn classify_chat_notification_normalisiert_und_ignoriert_unbekanntes() {
        // shared_chat_-Präfix wird entfernt (Python-Parität), Case egal.
        assert_eq!(
            classify_chat_notification("SHARED_CHAT_SUB"),
            Some(ChatNotificationKind::Sub)
        );
        assert_eq!(
            classify_chat_notification("  shared_chat_raid  "),
            Some(ChatNotificationKind::Raid)
        );
        // Unbekannter / leerer notice_type → None (sauberes Ignorieren, kein Panic).
        assert_eq!(classify_chat_notification("announcement"), None);
        assert_eq!(classify_chat_notification(""), None);
        assert_eq!(classify_chat_notification("   "), None);
    }


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
