from __future__ import annotations

import inspect
import logging
import time
from dataclasses import dataclass, field
from typing import Any, Awaitable, Callable

from .pending_raids import PendingRaid, PendingRaidStore, normalize_broadcaster_login


log = logging.getLogger("TwitchStreams.RaidManager")


async def _maybe_await(value: object) -> object:
    if inspect.isawaitable(value):
        return await value  # type: ignore[no-any-return]
    return value


@dataclass(slots=True)
class RaidTrackingRuntimeState:
    pending_store: PendingRaidStore = field(default_factory=PendingRaidStore)
    recent_raid_arrivals: dict[tuple[str, str], dict[str, Any]] = field(default_factory=dict)
    orphan_chat_raid_notifications: dict[tuple[str, str], dict[str, Any]] = field(default_factory=dict)
    readiness_states: dict[str, dict[str, Any]] = field(default_factory=dict)


SnapshotChatNotificationSubscriptionFn = Callable[[str], tuple[str | None, str | None]]
GetCogFn = Callable[[], Any | None]
EventSubHasSubFn = Callable[[Any, str, str], bool]
EnsureRaidTargetDynamicReadyFn = Callable[[Any, str, str, str | None], Awaitable[tuple[bool, str | None] | bool | tuple[bool, str]] | tuple[bool, str | None] | bool]
SubscribeRaidTargetDynamicFn = Callable[[Any, str, str], Awaitable[bool] | bool]
OrphanChatRaidNotificationHandlerFn = Callable[[dict[str, Any]], Awaitable[Any] | Any]
NextFlowIdFn = Callable[[str], str]
IncrementCounterFn = Callable[[str, int], int]
EmitRaidEventFn = Callable[..., Any]


@dataclass(slots=True)
class RaidTrackingRuntimeDependencies:
    logger: logging.Logger = field(default_factory=lambda: log)
    state: RaidTrackingRuntimeState = field(default_factory=RaidTrackingRuntimeState)
    snapshot_chat_notification_subscription: SnapshotChatNotificationSubscriptionFn | None = None
    get_cog: GetCogFn | None = None
    eventsub_has_sub: EventSubHasSubFn | None = None
    ensure_raid_target_dynamic_ready: EnsureRaidTargetDynamicReadyFn | None = None
    subscribe_raid_target_dynamic: SubscribeRaidTargetDynamicFn | None = None
    orphan_chat_raid_notification_handler: OrphanChatRaidNotificationHandlerFn | None = None
    next_raid_observability_flow_id: NextFlowIdFn = lambda prefix: f"{prefix}-{int(time.time() * 1000)}"
    increment_raid_observability_counter: IncrementCounterFn | None = None
    log_raid_observability_event: EmitRaidEventFn | None = None
    monotonic: Callable[[], float] = time.monotonic
    now: Callable[[], float] = time.time


@dataclass(slots=True)
class RaidTrackingRuntimeConfig:
    cleanup_timeout_seconds: float = 300.0
    pending_chat_notification_grace_seconds: float = 15.0
    orphan_chat_notification_retention_seconds: float = 900.0


class RaidTrackingRuntimeService:
    def __init__(
        self,
        dependencies: RaidTrackingRuntimeDependencies | None = None,
        config: RaidTrackingRuntimeConfig | None = None,
    ) -> None:
        self._deps = dependencies or RaidTrackingRuntimeDependencies()
        self._config = config or RaidTrackingRuntimeConfig()

    @property
    def state(self) -> RaidTrackingRuntimeState:
        return self._deps.state

    def cleanup_stale_pending_raids(
        self,
        *,
        now: float | None = None,
        timeout_seconds: float | None = None,
    ) -> list[PendingRaid]:
        current_time = float(now if now is not None else self._deps.now())
        timeout = float(timeout_seconds if timeout_seconds is not None else self._config.cleanup_timeout_seconds)
        stale = self.state.pending_store.cleanup_stale(timeout_seconds=timeout, now=current_time)
        for pending in stale:
            self._deps.increment_raid_observability_counter and self._deps.increment_raid_observability_counter(
                "raid_pending_timeout_total",
                1,
            )
            raid_flow_id = pending.raid_flow_id or self._next_flow_id(prefix="raid-timeout")
            age = current_time - float(pending.registered_ts or 0.0)
            offline_pending_s = (
                self._deps.monotonic() - float(pending.offline_trigger_ts)
                if pending.offline_trigger_ts is not None
                else -1.0
            )
            timeout_detail = self.build_pending_timeout_detail(pending)
            self._deps.logger.warning(
                "Pending raid timed out after %.0fs: %s -> (ID: %s). %s offline->pending=%.0fs",
                age,
                pending.from_broadcaster_login or "<unknown>",
                pending.to_broadcaster_id,
                timeout_detail,
                offline_pending_s,
            )
            self._emit_event(
                raid_flow_id=raid_flow_id,
                step="pending_timeout",
                decision="timeout",
                level=logging.WARNING,
                from_broadcaster_login=pending.from_broadcaster_login,
                to_broadcaster_id=pending.to_broadcaster_id,
                details={
                    "age_seconds": round(age, 1),
                    "offline_to_pending_seconds": round(offline_pending_s, 1),
                    "timeout_detail": timeout_detail,
                },
            )
        return stale

    def clear_superseded_pending_raids(
        self,
        *,
        from_broadcaster_login: str,
        current_target_id: str,
    ) -> list[PendingRaid]:
        normalized_from = normalize_broadcaster_login(from_broadcaster_login)
        if not normalized_from:
            return []

        superseded = self.state.pending_store.supersede_from_source(
            from_broadcaster_login=normalized_from,
            current_target_id=str(current_target_id or "").strip(),
        )
        for pending_record in superseded:
            target_stream_data = pending_record.target_stream_data
            old_target_login = ""
            if isinstance(target_stream_data, dict):
                old_target_login = normalize_broadcaster_login(target_stream_data.get("user_login"))
            raid_flow_id = pending_record.raid_flow_id or self._next_flow_id(prefix="raid-supersede")
            self._deps.logger.info(
                "Pending raid superseded before arrival: %s old_target=%s%s replaced_by=%s",
                from_broadcaster_login,
                pending_record.to_broadcaster_id,
                f" ({old_target_login})" if old_target_login else "",
                str(current_target_id or "").strip(),
            )
            self._emit_event(
                raid_flow_id=raid_flow_id,
                step="pending_superseded",
                decision="superseded",
                from_broadcaster_login=from_broadcaster_login,
                to_broadcaster_login=old_target_login or None,
                to_broadcaster_id=pending_record.to_broadcaster_id,
                details={"replaced_by": str(current_target_id or "").strip()},
            )
        return superseded

    def cancel_pending_raids_for_source_unraid(
        self,
        *,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None = None,
        message_id: str | None = None,
        event_timestamp: str | None = None,
    ) -> int:
        normalized_from = normalize_broadcaster_login(from_broadcaster_login)
        if not normalized_from:
            return 0

        canceled = 0
        for entry in list(self.state.pending_store.iter_entries()):
            pending = entry.raid
            if normalize_broadcaster_login(pending.from_broadcaster_login) != normalized_from:
                continue

            self._record_pending_signal_observation(
                pending,
                signal_type="channel.chat.notification.unraid_source",
                status="canceled",
                reason="source_self_unraid",
                detail=str(event_timestamp or message_id or "").strip() or None,
            )
            removed = self.state.pending_store.pop(
                to_broadcaster_id=entry.key[0],
                from_broadcaster_login=entry.key[1],
            )
            if removed is None:
                continue

            target_stream_data = pending.target_stream_data
            target_login = ""
            if isinstance(target_stream_data, dict):
                target_login = normalize_broadcaster_login(target_stream_data.get("user_login"))
            target_login = target_login or str(entry.key[0])
            raid_flow_id = pending.raid_flow_id or self._next_flow_id(prefix="raid-source-unraid")
            self._deps.increment_raid_observability_counter and self._deps.increment_raid_observability_counter(
                "raid_pending_canceled_source_unraid_total",
                1,
            )
            self._deps.logger.info(
                "Pending raid canceled by source unraid: %s -> %s (message_id=%s)",
                normalized_from,
                target_login,
                message_id or "n/a",
            )
            self._emit_event(
                raid_flow_id=raid_flow_id,
                step="pending_canceled_source_unraid",
                decision="canceled",
                from_broadcaster_login=normalized_from,
                from_broadcaster_id=from_broadcaster_id,
                to_broadcaster_login=target_login if target_login != str(entry.key[0]) else None,
                to_broadcaster_id=str(entry.key[0]),
                details={"message_id": message_id, "event_timestamp": event_timestamp},
            )
            canceled += 1

        return canceled

    async def ensure_raid_arrival_subscription_ready(
        self,
        *,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        raid_flow_id: str | None = None,
    ) -> bool:
        flow_id = str(raid_flow_id or "").strip() or self._next_flow_id(prefix="raid-ready")
        cog = self._deps.get_cog() if callable(self._deps.get_cog) else None
        if cog is None:
            self._emit_event(
                raid_flow_id=flow_id,
                step="readiness_check",
                decision="no_cog_best_effort",
                to_broadcaster_login=to_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
            )
            return True

        locally_tracked = False
        if callable(self._deps.eventsub_has_sub):
            try:
                locally_tracked = bool(self._deps.eventsub_has_sub(cog, "channel.raid", str(to_broadcaster_id)))
            except Exception:
                self._deps.logger.debug(
                    "EventSub channel.raid local tracking lookup failed for %s",
                    to_broadcaster_login,
                    exc_info=True,
                )

        ensure_ready = self._deps.ensure_raid_target_dynamic_ready
        if callable(ensure_ready):
            try:
                result = await _maybe_await(
                    ensure_ready(cog, str(to_broadcaster_id), to_broadcaster_login, flow_id)
                )
            except Exception:
                self._deps.increment_raid_observability_counter and self._deps.increment_raid_observability_counter(
                    "raid_eventsub_ready_check_failed_total",
                    1,
                )
                self._emit_event(
                    raid_flow_id=flow_id,
                    step="readiness_check",
                    decision="exception",
                    level=logging.ERROR,
                    to_broadcaster_login=to_broadcaster_login,
                    to_broadcaster_id=to_broadcaster_id,
                    details={"local_tracking": locally_tracked},
                )
                self._deps.logger.exception(
                    "EventSub channel.raid readiness check failed for %s",
                    to_broadcaster_login,
                )
                return False

            ready, detail = self._unpack_ready_result(result)
            self._remember_readiness_state(
                flow_id=flow_id,
                ready=ready,
                detail=str(detail or "").strip() or None,
                locally_tracked=bool(locally_tracked),
            )

            if ready:
                self._deps.increment_raid_observability_counter and self._deps.increment_raid_observability_counter(
                    "raid_eventsub_ready_true_total",
                    1,
                )
                self._deps.logger.info(
                    "EventSub channel.raid ready before raid start for %s (%s)",
                    to_broadcaster_login,
                    detail,
                )
            else:
                self._deps.increment_raid_observability_counter and self._deps.increment_raid_observability_counter(
                    "raid_eventsub_ready_false_total",
                    1,
                )
                if locally_tracked:
                    self._deps.increment_raid_observability_counter and self._deps.increment_raid_observability_counter(
                        "raid_eventsub_ready_false_local_true_total",
                        1,
                    )
                    detail = f"{detail}; local_tracking_only"
                self._deps.logger.warning(
                    "EventSub channel.raid not confirmed enabled for %s before raid start (%s). Proceeding best-effort.",
                    to_broadcaster_login,
                    detail,
                )
            self._emit_event(
                raid_flow_id=flow_id,
                step="readiness_check",
                decision="ready" if ready else "not_ready",
                level=logging.INFO if ready else logging.WARNING,
                to_broadcaster_login=to_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
                details={"local_tracking": locally_tracked, "detail": detail},
            )
            return ready

        if locally_tracked:
            self._deps.logger.debug(
                "EventSub channel.raid for %s is only locally tracked; remote readiness check unavailable.",
                to_broadcaster_login,
            )
        self._remember_readiness_state(
            flow_id=flow_id,
            ready=True,
            detail="local_tracking_only" if locally_tracked else "best_effort",
            locally_tracked=bool(locally_tracked),
        )
        self._emit_event(
            raid_flow_id=flow_id,
            step="readiness_check",
            decision="best_effort",
            to_broadcaster_login=to_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            details={"local_tracking": locally_tracked},
        )
        return True

    async def register_pending_raid(
        self,
        *,
        from_broadcaster_login: str,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        target_stream_data: dict | None = None,
        is_partner_raid: bool = False,
        viewer_count: int = 0,
        offline_trigger_ts: float | None = None,
        raid_flow_id: str | None = None,
        channel_raid_ready: bool | None = None,
    ) -> PendingRaid | None:
        chat_notification_state, chat_notification_detail = self._snapshot_chat_notification_subscription(
            to_broadcaster_login
        )
        flow_id = str(raid_flow_id or "").strip() or self._next_flow_id(prefix="raid-pending")
        readiness_state = self._pop_readiness_state(flow_id)
        self.clear_superseded_pending_raids(
            from_broadcaster_login=from_broadcaster_login,
            current_target_id=to_broadcaster_id,
        )
        pending_record = self._build_pending_raid_record(
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            target_stream_data=target_stream_data,
            is_partner_raid=is_partner_raid,
            viewer_count=viewer_count,
            offline_trigger_ts=offline_trigger_ts,
            raid_flow_id=flow_id,
            channel_raid_ready=channel_raid_ready,
            channel_raid_ready_detail=str(readiness_state.get("detail") or "").strip() or None,
            chat_notification_state=chat_notification_state,
            chat_notification_detail=chat_notification_detail,
        )
        self.state.pending_store.store(pending_record)
        self._deps.increment_raid_observability_counter and self._deps.increment_raid_observability_counter(
            "raid_pending_registered_total",
            1,
        )
        offline_to_pending_ms = (
            (self._deps.monotonic() - offline_trigger_ts) * 1000 if offline_trigger_ts else None
        )
        self._deps.logger.info(
            "Pending raid registered: %s -> %s (ID: %s). Creating EventSub subscription... offline->pending=%s, chat_notification=%s",
            from_broadcaster_login,
            to_broadcaster_login,
            to_broadcaster_id,
            f"{offline_to_pending_ms:.0f}ms" if offline_to_pending_ms is not None else "n/a",
            chat_notification_state or "unknown",
        )
        self._emit_event(
            raid_flow_id=flow_id,
            step="pending_registered",
            decision="registered",
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_login=to_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            details={
                "viewer_count": viewer_count,
                "offline_to_pending_ms": int(offline_to_pending_ms) if offline_to_pending_ms is not None else None,
                "channel_raid_ready": channel_raid_ready,
                "channel_raid_ready_detail": readiness_state.get("detail"),
                "chat_notification_state": chat_notification_state,
                "chat_notification_detail": chat_notification_detail,
            },
        )

        success = bool(channel_raid_ready)
        if success:
            self._deps.logger.debug(
                "EventSub channel.raid readiness already confirmed for %s - skipping duplicate create",
                to_broadcaster_login,
            )

        if not success and callable(self._deps.get_cog):
            cog = self._deps.get_cog()
            if cog is not None and callable(self._deps.subscribe_raid_target_dynamic):
                try:
                    self._deps.increment_raid_observability_counter and self._deps.increment_raid_observability_counter(
                        "raid_eventsub_subscribe_attempt_total",
                        1,
                    )
                    success = bool(
                        await _maybe_await(
                            self._deps.subscribe_raid_target_dynamic(
                                cog, str(to_broadcaster_id), to_broadcaster_login
                            )
                        )
                    )
                    if success:
                        self._deps.increment_raid_observability_counter and self._deps.increment_raid_observability_counter(
                            "raid_eventsub_subscribe_success_total",
                            1,
                        )
                        self._deps.logger.info(
                            "EventSub channel.raid subscription created for %s",
                            to_broadcaster_login,
                        )
                    else:
                        self._deps.increment_raid_observability_counter and self._deps.increment_raid_observability_counter(
                            "raid_eventsub_subscribe_failed_total",
                            1,
                        )
                        self._deps.logger.warning(
                            "Failed to create EventSub subscription for %s - raid message may not be sent",
                            to_broadcaster_login,
                        )
                except Exception:
                    self._deps.increment_raid_observability_counter and self._deps.increment_raid_observability_counter(
                        "raid_eventsub_subscribe_failed_total",
                        1,
                    )
                    self._deps.logger.exception(
                        "Error creating dynamic EventSub subscription for %s",
                        to_broadcaster_login,
                    )
            elif not success:
                self._deps.logger.warning(
                    "Cog reference not set - cannot create dynamic EventSub subscription for %s",
                    to_broadcaster_login,
                )
        self._emit_event(
            raid_flow_id=flow_id,
            step="pending_subscription_create",
            decision="created" if success else "best_effort_only",
            level=logging.INFO if success else logging.WARNING,
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_login=to_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            details={"channel_raid_ready": channel_raid_ready},
        )

        orphan_notification = self.pop_orphan_chat_raid_notification(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )
        if orphan_notification:
            self._deps.increment_raid_observability_counter and self._deps.increment_raid_observability_counter(
                "raid_orphan_chat_notification_total",
                1,
            )
            self._deps.logger.info(
                "Pending raid %s -> %s matched earlier channel.chat.notification raid signal.",
                from_broadcaster_login,
                to_broadcaster_login,
            )
            self._emit_event(
                raid_flow_id=flow_id,
                step="pending_orphan_notification_match",
                decision="matched",
                from_broadcaster_login=from_broadcaster_login,
                to_broadcaster_login=to_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
                details={"message_id": orphan_notification.get("message_id")},
            )
            handler = self._deps.orphan_chat_raid_notification_handler
            if callable(handler):
                await _maybe_await(
                    handler(
                        {
                            "to_broadcaster_id": str(orphan_notification.get("to_broadcaster_id") or to_broadcaster_id),
                            "to_broadcaster_login": str(orphan_notification.get("to_broadcaster_login") or to_broadcaster_login),
                            "from_broadcaster_login": str(orphan_notification.get("from_broadcaster_login") or from_broadcaster_login),
                            "viewer_count": int(orphan_notification.get("viewer_count") or viewer_count),
                            "from_broadcaster_id": str(orphan_notification.get("from_broadcaster_id") or "") or None,
                            "message_id": str(orphan_notification.get("message_id") or "") or None,
                            "event_timestamp": str(orphan_notification.get("event_timestamp") or "") or None,
                        }
                    )
                )

        return pending_record

    def build_pending_timeout_detail(self, pending_record: PendingRaid) -> str:
        observation_parts: list[str] = []
        for signal_type in ("channel.raid", "channel.chat.notification"):
            observation = pending_record.signal_observations.get(signal_type)
            if not isinstance(observation, dict):
                continue
            status = str(observation.get("status") or "").strip()
            reason = str(observation.get("reason") or "").strip()
            detail = str(observation.get("detail") or "").strip()
            text = f"{signal_type}:{status}" if status else signal_type
            if reason:
                text += f" ({reason})"
            if detail:
                text += f" [{detail}]"
            observation_parts.append(text)

        if not observation_parts:
            channel_raid_ready = pending_record.channel_raid_ready
            channel_raid_detail = (
                "ready" if channel_raid_ready is not False else "subscription_not_ready"
            )
            chat_state = str(pending_record.chat_notification_state or "").strip()
            chat_detail = str(pending_record.chat_notification_detail or "").strip()
            if not chat_state:
                chat_state = "missing"
            chat_text = f"channel.chat.notification:{chat_state}"
            if chat_detail:
                chat_text += f" [{chat_detail}]"
            observation_parts.extend(
                [
                    f"channel.raid:{channel_raid_detail}",
                    chat_text,
                ]
            )

        return "Timeout detail: " + "; ".join(observation_parts)

    def store_orphan_chat_raid_notification(self, payload: dict[str, Any]) -> None:
        key = self._build_raid_arrival_cache_key(
            to_broadcaster_id=str(payload.get("to_broadcaster_id") or "").strip(),
            from_broadcaster_login=str(payload.get("from_broadcaster_login") or "").strip(),
        )
        payload_copy = dict(payload)
        payload_copy["observed_ts"] = float(payload_copy.get("observed_ts") or self._deps.now())
        self.state.orphan_chat_raid_notifications[key] = payload_copy

    def pop_orphan_chat_raid_notification(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str,
    ) -> dict[str, Any] | None:
        key = self._build_raid_arrival_cache_key(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )
        return self.state.orphan_chat_raid_notifications.pop(key, None)

    def _build_pending_raid_record(
        self,
        *,
        from_broadcaster_login: str,
        to_broadcaster_id: str,
        target_stream_data: dict | None,
        is_partner_raid: bool,
        viewer_count: int,
        offline_trigger_ts: float | None,
        raid_flow_id: str | None = None,
        channel_raid_ready: bool | None = None,
        channel_raid_ready_detail: str | None = None,
        chat_notification_state: str | None = None,
        chat_notification_detail: str | None = None,
    ) -> PendingRaid:
        return PendingRaid(
            from_broadcaster_login=normalize_broadcaster_login(from_broadcaster_login),
            to_broadcaster_id=str(to_broadcaster_id or "").strip(),
            target_stream_data=dict(target_stream_data) if isinstance(target_stream_data, dict) else None,
            registered_ts=self._deps.now(),
            is_partner_raid=bool(is_partner_raid),
            registered_viewer_count=int(viewer_count or 0),
            offline_trigger_ts=float(offline_trigger_ts) if offline_trigger_ts else None,
            raid_flow_id=str(raid_flow_id or "").strip() or None,
            channel_raid_ready=channel_raid_ready,
            channel_raid_ready_detail=str(channel_raid_ready_detail or "").strip() or None,
            chat_notification_state=str(chat_notification_state or "").strip() or None,
            chat_notification_detail=str(chat_notification_detail or "").strip() or None,
        ).normalize()

    @staticmethod
    def _build_raid_arrival_cache_key(
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str,
    ) -> tuple[str, str]:
        return (
            str(to_broadcaster_id or "").strip(),
            normalize_broadcaster_login(from_broadcaster_login),
        )

    def _remember_readiness_state(
        self,
        *,
        flow_id: str,
        ready: bool,
        detail: str | None,
        locally_tracked: bool,
    ) -> None:
        self.state.readiness_states[str(flow_id or "").strip()] = {
            "ready": bool(ready),
            "detail": detail,
            "locally_tracked": bool(locally_tracked),
        }

    def _pop_readiness_state(self, flow_id: str) -> dict[str, Any]:
        return self.state.readiness_states.pop(str(flow_id or "").strip(), {})

    def _snapshot_chat_notification_subscription(
        self,
        broadcaster_login: str,
    ) -> tuple[str | None, str | None]:
        callback = self._deps.snapshot_chat_notification_subscription
        if callable(callback):
            try:
                state = callback(broadcaster_login)
            except Exception:
                self._deps.logger.debug(
                    "Could not resolve channel.chat.notification subscription state for %s",
                    broadcaster_login,
                    exc_info=True,
                )
            else:
                state_value = None
                detail_value = None
                if len(state) > 0 and state[0] is not None:
                    state_value = str(state[0]).strip() or None
                if len(state) > 1 and state[1] is not None:
                    detail_value = str(state[1]).strip() or None
                return state_value, detail_value
        return "not_joined", "channel.chat.notification not subscribed"

    def _record_pending_signal_observation(
        self,
        pending_record: PendingRaid,
        *,
        signal_type: str,
        status: str,
        reason: str | None = None,
        detail: str | None = None,
    ) -> None:
        pending_record.record_signal_observation(
            signal_type=signal_type,
            status=status,
            reason=reason,
            detail=detail,
        )

    def _next_flow_id(self, *, prefix: str) -> str:
        try:
            flow_id = self._deps.next_raid_observability_flow_id(prefix)
        except Exception:
            flow_id = ""
        return str(flow_id or "").strip() or f"{prefix}-{int(self._deps.monotonic() * 1000)}"

    def _emit_event(self, **payload: Any) -> None:
        emit = self._deps.log_raid_observability_event
        if not callable(emit):
            return
        try:
            emit(**payload)
        except Exception:
            self._deps.logger.debug("Raid tracking runtime observability event failed", exc_info=True)

    @staticmethod
    def _unpack_ready_result(result: object) -> tuple[bool, str | None]:
        if isinstance(result, tuple) and result:
            ready = bool(result[0])
            detail = str(result[1]).strip() if len(result) > 1 and result[1] is not None else None
            return ready, detail or None
        return bool(result), None


__all__ = [
    "RaidTrackingRuntimeConfig",
    "RaidTrackingRuntimeDependencies",
    "RaidTrackingRuntimeService",
    "RaidTrackingRuntimeState",
]
