//! Verarbeitung der Core-EventSub-Aufträge aus der Processing-Inbox
//! (`stream.online` / `stream.offline` / `channel.update` / `channel.raid`) — Port der
//! Python-Handler aus `analytics/mixin.py` und `eventsub_mixin.py`.
//!
//! Fachliche Effekte sind über Business-Effect-Guards exactly-once pro
//! Message (`{effekt}:{message_id}`, TTL 7 Tage, Release bei Fehler);
//! `stream.offline` ist zusätzlich pro Broadcaster gegen Flapping gedrosselt
//! (120 s). Subsystemfremde Folgeeffekte laufen über [`EventSubHooks`].

use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;

use crate::dispatch::EventSubHooks;
use crate::guard::{GuardKind, GuardStore};
use crate::inbox_runtime::{ClockFn, HandlerError, InboxHandler};
use crate::live_state::LiveStateStore;
use crate::poller::hooks::announcement_reannounce_cooldown_key;
use crate::poller::source::ChannelInfoSource;
use crate::sessions::SessionTracker;
use crate::stream::{iso_seconds, StreamSnapshot};
use crate::telemetry::TelemetryStore;

#[derive(Debug, Clone, PartialEq, Eq)]
enum StreamOnlineAnnouncementAction {
    Keep,
    Clear {
        expected_message_id: String,
    },
    /// Nachricht und alte Stream-ID behalten, damit der Poller sie vor einem
    /// moeglichen Neu-Post zuerst sicher bearbeiten kann.
    Reconcile,
}

/// Business-Effect-TTL (Python: 7 Tage).
pub const BUSINESS_EFFECT_TTL_SECONDS: f64 = 7.0 * 24.0 * 3600.0;
/// Offline-Drossel gegen Doppel-Trigger Polling/EventSub (Python: 120 s).
pub const OFFLINE_THROTTLE_TTL_SECONDS: f64 = 120.0;

/// Führt einen fachlichen Effekt exactly-once pro Message aus
/// (Python `_run_eventsub_business_effect_once`). Ohne message_id läuft der
/// Effekt direkt; bei Fehlern wird der Guard wieder freigegeben.
pub async fn run_business_effect_once<F, Fut>(
    guard: &GuardStore,
    message_id: Option<&str>,
    effect_name: &str,
    now: f64,
    effect: F,
) -> Result<bool, HandlerError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), HandlerError>>,
{
    let Some(message_id) = message_id.map(str::trim).filter(|m| !m.is_empty()) else {
        effect().await?;
        return Ok(true);
    };
    let guard_key = format!("{}:{}", effect_name.trim().to_lowercase(), message_id);
    let claimed = guard
        .claim(
            GuardKind::BusinessEffect,
            &guard_key,
            BUSINESS_EFFECT_TTL_SECONDS,
            now,
        )
        .await
        .map_err(|e| Box::new(e) as HandlerError)?;
    if !claimed {
        return Ok(false);
    }
    if let Err(error) = effect().await {
        if let Err(release_error) = guard.release(GuardKind::BusinessEffect, &guard_key).await {
            tracing::warn!(
                %release_error,
                guard_key,
                "EventSub BusinessEffect-Guard konnte nach Fehler nicht freigegeben werden"
            );
        }
        return Err(error);
    }
    Ok(true)
}

/// Inbox-Handler für die Monitoring-Core-Events.
pub struct MonitoringEventHandler {
    guard: GuardStore,
    live_state: LiveStateStore,
    tracker: Arc<SessionTracker>,
    telemetry: TelemetryStore,
    hooks: Arc<dyn EventSubHooks>,
    channel_info: Option<Arc<dyn ChannelInfoSource>>,
    clock: ClockFn,
}

impl MonitoringEventHandler {
    pub fn new(
        guard: GuardStore,
        live_state: LiveStateStore,
        tracker: Arc<SessionTracker>,
        telemetry: TelemetryStore,
        hooks: Arc<dyn EventSubHooks>,
        channel_info: Option<Arc<dyn ChannelInfoSource>>,
        clock: ClockFn,
    ) -> Self {
        Self {
            guard,
            live_state,
            tracker,
            telemetry,
            hooks,
            channel_info,
            clock,
        }
    }

    fn now_pair(&self) -> (f64, DateTime<Utc>) {
        let epoch = (self.clock)();
        let dt = Utc
            .timestamp_opt(epoch as i64, 0)
            .single()
            .unwrap_or_else(Utc::now);
        (epoch, dt)
    }

    async fn announcement_action_on_stream_online(
        &self,
        broadcaster_id: &str,
        login: &str,
        stream_id: Option<&str>,
        epoch: f64,
    ) -> StreamOnlineAnnouncementAction {
        let state = match self
            .live_state
            .online_announcement_state(broadcaster_id)
            .await
        {
            Ok(Some(state)) => state,
            Ok(None) => return StreamOnlineAnnouncementAction::Keep,
            Err(error) => {
                tracing::debug!(%error, broadcaster_id, "stream.online: Announcement-State nicht lesbar");
                return StreamOnlineAnnouncementAction::Keep;
            }
        };
        let previous_message_id = state
            .last_discord_message_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty());
        let Some(previous_message_id) = previous_message_id else {
            return StreamOnlineAnnouncementAction::Keep;
        };

        let previous_stream_id = state
            .last_stream_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty());
        let current_stream_id = stream_id.map(str::trim).filter(|id| !id.is_empty());
        let stream_changed = current_stream_id.is_some() && previous_stream_id != current_stream_id;
        let unresolved_action = if stream_changed {
            StreamOnlineAnnouncementAction::Reconcile
        } else {
            StreamOnlineAnnouncementAction::Keep
        };

        if state.is_live.unwrap_or(0) != 0 {
            return unresolved_action;
        }

        let Some(key) = announcement_reannounce_cooldown_key(login) else {
            return unresolved_action;
        };
        let has_reconnect_marker = match self.guard.has_entry(GuardKind::BusinessEffect, &key).await
        {
            Ok(has_entry) => has_entry,
            Err(error) => {
                tracing::debug!(%error, login, "stream.online: Reannounce-Cooldown-Marker nicht lesbar");
                return unresolved_action;
            }
        };
        if has_reconnect_marker {
            return match self
                .guard
                .is_active(GuardKind::BusinessEffect, &key, epoch)
                .await
            {
                Ok(true) => StreamOnlineAnnouncementAction::Keep,
                Ok(false) => StreamOnlineAnnouncementAction::Clear {
                    expected_message_id: previous_message_id.to_string(),
                },
                Err(error) => {
                    tracing::debug!(%error, login, "stream.online: Reannounce-Cooldown nicht lesbar");
                    unresolved_action
                }
            };
        }

        unresolved_action
    }

    /// stream.online (Python `_handle_stream_online` + Followups):
    /// minimaler Live-State + Go-Live-Hook + Score-Refresh, beide
    /// exactly-once pro Message.
    async fn handle_stream_online(&self, work: &WorkPayload) -> Result<(), HandlerError> {
        let (epoch, now) = self.now_pair();
        let started_at = work
            .event
            .get("started_at")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let stream_id = ["id", "stream_id"]
            .iter()
            .find_map(|k| work.event.get(*k))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let login = work.login_lower();
        let announcement_action = self
            .announcement_action_on_stream_online(&work.broadcaster_id, &login, stream_id, epoch)
            .await;
        let stream_id_to_store = match &announcement_action {
            StreamOnlineAnnouncementAction::Reconcile => None,
            StreamOnlineAnnouncementAction::Keep => stream_id,
            StreamOnlineAnnouncementAction::Clear { .. } => stream_id,
        };
        let started_at_to_store = match &announcement_action {
            StreamOnlineAnnouncementAction::Reconcile => None,
            StreamOnlineAnnouncementAction::Keep | StreamOnlineAnnouncementAction::Clear { .. } => {
                started_at
            }
        };
        let clear_expected_message_id = match &announcement_action {
            StreamOnlineAnnouncementAction::Clear {
                expected_message_id,
            } => Some(expected_message_id.as_str()),
            StreamOnlineAnnouncementAction::Keep | StreamOnlineAnnouncementAction::Reconcile => {
                None
            }
        };
        self.live_state
            .apply_stream_online(
                &work.broadcaster_id,
                &login,
                stream_id_to_store,
                started_at_to_store,
                &iso_seconds(now),
                clear_expected_message_id,
            )
            .await
            .map_err(|e| Box::new(e) as HandlerError)?;
        self.guard
            .release(GuardKind::OfflineThrottle, &work.broadcaster_id)
            .await
            .map_err(|e| Box::new(e) as HandlerError)?;
        // WIRING-TODO(P2.55): Weitere Go-Live-Adapter außerhalb der
        // EventSub-Inbox (z. B. bin/tb-bot-Hooks) müssen dieselbe
        // OfflineThrottle-Freigabe nutzen, falls sie eigene Go-Live-Pfade
        // ausführen.

        // Session sofort bei stream.online eroeffnen statt erst beim naechsten
        // Poll-Tick: sonst geht der gesamte Go-Live-Chat (bis poll_interval +
        // negativem Session-Cache) still verloren, weil Chatter ohne offene
        // Session verworfen werden. Idempotent: run_business_effect_once
        // (message_id) + start_session advisory-lock/AlreadyOpen verhindern eine
        // Doppel-Session gegen den Poll-Pfad; fehlende Felder (Titel/Game/
        // Viewer) backfillt der erste Poll via adopt_incomplete. Live-State
        // steht bereits (oben), damit der Chat-Game-Gate ihn lesen kann.
        run_business_effect_once(
            &self.guard,
            work.message_id.as_deref(),
            "stream_online_session",
            epoch,
            || async {
                let snapshot = StreamSnapshot {
                    id: stream_id.map(str::to_string),
                    started_at: started_at.map(str::to_string),
                    ..Default::default()
                };
                if let Some(session_id) = self
                    .tracker
                    .ensure_session(
                        &login,
                        &snapshot,
                        None,
                        Some(work.broadcaster_id.as_str()),
                        now,
                    )
                    .await
                {
                    tracing::info!(
                        login = %login,
                        session_id,
                        "EventSub stream.online: Session sofort eroeffnet"
                    );
                }
                Ok(())
            },
        )
        .await?;

        // Go-Live-Enrichment: Kategorie/Titel sofort per gezieltem
        // /channels-Lookup setzen (sprachfilter-frei, kein Helix-Lag wie bei
        // /streams). Best-Effort: Fehler loggen statt failen, das Polling
        // füllt die Felder sonst beim nächsten Tick nach.
        if announcement_action != StreamOnlineAnnouncementAction::Reconcile {
            if let Some(source) = &self.channel_info {
                run_business_effect_once(
                    &self.guard,
                    work.message_id.as_deref(),
                    "stream_online_channel_info",
                    epoch,
                    || async {
                        match source.channel_info(&work.broadcaster_id).await {
                            Ok(Some(info)) => {
                                if let Err(error) = self
                                    .live_state
                                    .apply_channel_info(
                                        &work.broadcaster_id,
                                        info.title.as_deref(),
                                        info.game_name.as_deref(),
                                    )
                                    .await
                                {
                                    tracing::warn!(
                                        %error,
                                        broadcaster_id = %work.broadcaster_id,
                                        "Go-Live-Enrichment: Live-State-Update fehlgeschlagen"
                                    );
                                }
                            }
                            Ok(None) => {}
                            Err(error) => {
                                tracing::warn!(
                                    %error,
                                    broadcaster_id = %work.broadcaster_id,
                                    "Go-Live-Enrichment: Kanal-Lookup fehlgeschlagen"
                                );
                            }
                        }
                        Ok(())
                    },
                )
                .await?;
            }
        }

        self.run_stream_online_followups(
            &work.broadcaster_id,
            &login,
            stream_id,
            work.message_id.as_deref(),
        )
        .await?;
        Ok(())
    }

    async fn run_stream_online_followups(
        &self,
        broadcaster_id: &str,
        login: &str,
        stream_id: Option<&str>,
        message_id: Option<&str>,
    ) -> Result<(), HandlerError> {
        let (epoch, _) = self.now_pair();
        let executed = run_business_effect_once(
            &self.guard,
            message_id,
            "stream_online_went_live",
            epoch,
            || async {
                self.hooks
                    .on_stream_went_live_with_stream_id(broadcaster_id, login, stream_id)
                    .await;
                Ok(())
            },
        )
        .await?;
        if executed {
            tracing::info!(
                login = %login,
                broadcaster_id,
                "EventSub stream.online: Go-Live-Handler getriggert"
            );
        }
        run_business_effect_once(
            &self.guard,
            message_id,
            "stream_online_refresh",
            epoch,
            || async {
                self.hooks
                    .on_score_refresh(broadcaster_id, Some(login), "eventsub_stream_online")
                    .await;
                Ok(())
            },
        )
        .await?;
        Ok(())
    }

    async fn handle_stream_online_followups(&self, payload: &Value) -> Result<(), HandlerError> {
        let text = |key: &str| {
            payload
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("")
                .to_string()
        };
        let broadcaster_id = {
            let id = text("broadcaster_user_id");
            if id.is_empty() {
                text("broadcaster_id")
            } else {
                id
            }
        };
        if broadcaster_id.is_empty() {
            return Err("invalid stream.online.followups processing payload".into());
        }
        let broadcaster_login = text("broadcaster_login");
        let login_value = text("login_value").to_lowercase();
        let login = if login_value.is_empty() {
            broadcaster_login.to_lowercase()
        } else {
            login_value
        };
        let stream_id = text("stream_id");
        let stream_id = Some(stream_id.as_str()).filter(|s| !s.is_empty());
        let message_id = text("message_id");
        let message_id = Some(message_id.as_str()).filter(|m| !m.is_empty());

        self.run_stream_online_followups(&broadcaster_id, &login, stream_id, message_id)
            .await
    }

    /// stream.offline (Python `_on_eventsub_stream_offline`). Reihenfolge der
    /// Seiteneffekte 1:1 zum Orakel (`eventsub_mixin.py`):
    /// 1. **Engagement-Auto-Off** — VOR dem Throttle (läuft auch bei Duplikat).
    /// 2. **Offline-Throttle** (120s) — Duplikat → früher Ausstieg.
    /// 3. **Global-Ban-Sweep** — nach Throttle, VOR State-Finalize.
    /// 4. **State-Finalize** (Session + Live-State offline, exactly-once).
    /// 5. **Score-Refresh + Auto-Raid + Post-Stream** via on_stream_offline.
    async fn handle_stream_offline(&self, work: &WorkPayload) -> Result<(), HandlerError> {
        if work.broadcaster_id.is_empty() {
            return Ok(());
        }
        let (epoch, now) = self.now_pair();
        let login = match work.login_lower() {
            l if l.is_empty() => self
                .live_state
                .login_for_user_id(&work.broadcaster_id)
                .await
                .ok()
                .flatten()
                .unwrap_or_default(),
            l => l,
        };
        let login_opt = Some(&login).filter(|l| !l.is_empty()).map(|l| l.as_str());

        // (1) Engagement-Auto-Off VOR dem Throttle: idempotentes UPDATE, das
        // den Engagement-Layer auch bei einem gedrosselten Duplikat-Offline ans
        // Stream-Leben koppelt (Python `eventsub_mixin.py`:1861).
        self.hooks
            .on_stream_offline_engagement(&work.broadcaster_id, login_opt)
            .await;

        let claimed = self
            .guard
            .claim(
                GuardKind::OfflineThrottle,
                &work.broadcaster_id,
                OFFLINE_THROTTLE_TTL_SECONDS,
                epoch,
            )
            .await
            .map_err(|e| Box::new(e) as HandlerError)?;
        if !claimed {
            tracing::debug!(
                broadcaster_id = %work.broadcaster_id,
                "EventSub Offline-Throttle: noch im 120s-Fenster, ignoriere"
            );
            return Ok(());
        }

        // (3) Global-Ban-Sweep nach bestandenem Throttle, VOR State-Finalize
        // (Python `eventsub_mixin.py`:1901).
        self.hooks
            .on_stream_offline_global_ban(&work.broadcaster_id, login_opt)
            .await;

        run_business_effect_once(
            &self.guard,
            work.message_id.as_deref(),
            "stream_offline_state",
            epoch,
            || async {
                if !login.is_empty() {
                    self.tracker
                        .finalize(&login, "offline", None, Some(now))
                        .await;
                }
                self.live_state
                    .apply_stream_offline(&work.broadcaster_id, &iso_seconds(now))
                    .await
                    .map_err(|e| Box::new(e) as HandlerError)
            },
        )
        .await?;

        run_business_effect_once(
            &self.guard,
            work.message_id.as_deref(),
            "stream_offline_refresh",
            epoch,
            || async {
                self.hooks
                    .on_score_refresh(&work.broadcaster_id, login_opt, "eventsub_stream_offline")
                    .await;
                Ok(())
            },
        )
        .await?;

        run_business_effect_once(
            &self.guard,
            work.message_id.as_deref(),
            "stream_offline_auto_raid",
            epoch,
            || async {
                self.hooks
                    .on_stream_offline(&work.broadcaster_id, login_opt)
                    .await;
                Ok(())
            },
        )
        .await?;
        Ok(())
    }

    /// channel.update (Python `_handle_channel_update`): Protokoll +
    /// Live-State (exactly-once) + Score-Refresh.
    async fn handle_channel_update(&self, work: &WorkPayload) -> Result<(), HandlerError> {
        let (epoch, now) = self.now_pair();
        run_business_effect_once(
            &self.guard,
            work.message_id.as_deref(),
            "channel_update_db",
            epoch,
            || async {
                self.telemetry
                    .store_channel_update(&work.broadcaster_id, &work.event, now)
                    .await
                    .map_err(|e| Box::new(e) as HandlerError)
            },
        )
        .await?;
        run_business_effect_once(
            &self.guard,
            work.message_id.as_deref(),
            "channel_update_refresh",
            epoch,
            || async {
                self.hooks
                    .on_score_refresh(&work.broadcaster_id, None, "eventsub_channel_update")
                    .await;
                Ok(())
            },
        )
        .await?;
        Ok(())
    }

    /// channel.raid: durable Inbox-Ausführung des Raid-Arrival-Hooks. Der
    /// Hook-Return ist aktuell `()`, daher liefert die Inbox heute vor allem
    /// Cross-Restart-Durability.
    ///
    /// WIRING-TODO(P2.52/P2.54): Wenn `bin/tb-bot/src/eventsub_hooks.rs`
    /// (`RaidArrivalCoordinator`) Hook-Fehler retrybar signalisieren soll, muss
    /// der konkrete Adapter auf einen Result-fähigen Pfad erweitert werden.
    async fn handle_channel_raid(&self, work: &WorkPayload) -> Result<(), HandlerError> {
        let (epoch, _) = self.now_pair();
        let executed = run_business_effect_once(
            &self.guard,
            work.message_id.as_deref(),
            "channel_raid_arrival",
            epoch,
            || async {
                self.hooks
                    .on_channel_raid(&work.event, work.message_id.as_deref())
                    .await;
                Ok(())
            },
        )
        .await?;
        if executed {
            tracing::info!(
                broadcaster_id = %work.broadcaster_id,
                "EventSub channel.raid: Raid-Arrival-Handler getriggert"
            );
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl InboxHandler for MonitoringEventHandler {
    async fn handle(&self, work_type: &str, payload: &Value) -> Result<(), HandlerError> {
        let work = WorkPayload::from(payload);
        match work_type.trim().to_lowercase().as_str() {
            "stream.online" => self.handle_stream_online(&work).await,
            "stream.online.followups" => self.handle_stream_online_followups(payload).await,
            "stream.offline" => self.handle_stream_offline(&work).await,
            "channel.update" => self.handle_channel_update(&work).await,
            "channel.raid" => self.handle_channel_raid(&work).await,
            other => Err(format!("unknown eventsub processing work_type: {other}").into()),
        }
    }
}

/// Entpackter Inbox-Payload (vom Dispatcher erzeugt).
struct WorkPayload {
    broadcaster_id: String,
    broadcaster_login: String,
    message_id: Option<String>,
    event: Value,
}

impl WorkPayload {
    fn from(payload: &Value) -> Self {
        let text = |key: &str| {
            payload
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("")
                .to_string()
        };
        Self {
            broadcaster_id: text("broadcaster_id"),
            broadcaster_login: text("broadcaster_login"),
            message_id: Some(text("message_id")).filter(|m| !m.is_empty()),
            event: payload.get("event").cloned().unwrap_or(Value::Null),
        }
    }

    fn login_lower(&self) -> String {
        let login = self.broadcaster_login.trim().to_lowercase();
        if !login.is_empty() {
            return login;
        }
        self.event
            .get("broadcaster_user_login")
            .and_then(Value::as_str)
            .map(|l| l.trim().to_lowercase())
            .unwrap_or_default()
    }
}
