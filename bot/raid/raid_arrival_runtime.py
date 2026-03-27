from __future__ import annotations

import asyncio
import logging
import time
from dataclasses import dataclass, field
from typing import Any, Awaitable, Callable

from .arrival_confirmation import ArrivalConfirmationService
from .pending_raids import PendingRaid, normalize_broadcaster_login
from .signal_correlation import RaidSignalAction, RaidSignalCorrelationService

_PENDING_CHAT_NOTIFICATION_GRACE_SECONDS = 15.0


@dataclass(slots=True, frozen=True)
class RaidArrivalRuntimeDependencies:
    arrival_confirmation_service: ArrivalConfirmationService
    signal_correlation_service: RaidSignalCorrelationService
    get_pending_raid: Callable[[str, str], PendingRaid | None]
    store_pending_raid: Callable[[PendingRaid], Any]
    pop_pending_raid: Callable[[str, str], PendingRaid | None]
    record_pending_signal_observation: Callable[
        [PendingRaid, str, str, str | None, str | None],
        Any,
    ]
    store_orphan_chat_raid_notification: Callable[[dict[str, Any]], Any]
    lookup_recent_raid_arrival: Callable[[str, str], dict[str, Any] | None]
    remember_recent_raid_arrival: Callable[..., Any]
    update_partner_raid_arrival: Callable[[int, set[str], bool], Any]
    store_partner_raid_arrival: Callable[..., int | None]
    load_recent_raid_history_reference: Callable[[str, str], tuple[int | None, str | None]]
    process_independent_partner_raid_arrival: Callable[..., bool]
    cancel_pending_raids_for_source_unraid: Callable[..., int]
    resolve_streamer_id_by_login: Callable[[str], str | None]
    mark_manual_raid_started: Callable[[str, float], Any]
    lookup_silent_raid_enabled: Callable[[str], bool]
    refresh_partner_score_cache_if_available: Callable[[str], Awaitable[Any]]
    track_confirmed_partner_raid: Callable[..., Any] | None
    delete_external_recruitment_blacklist_pending: Callable[[str], Any]
    record_confirmed_external_recruitment_raid: Callable[..., int | None]
    maybe_schedule_external_recruitment_blacklist_pending: Callable[..., Any]
    send_partner_raid_message: Callable[..., Awaitable[Any]]
    send_recruitment_message: Callable[..., Awaitable[Any]]
    increment_raid_observability_counter: Callable[[str, int], int]
    log_raid_observability_event: Callable[..., Any]
    next_raid_observability_flow_id: Callable[[str], str]
    logger: logging.Logger = field(default_factory=lambda: logging.getLogger("TwitchStreams.RaidManager"))
    now: Callable[[], float] = time.time


class RaidArrivalRuntime:
    def __init__(self, dependencies: RaidArrivalRuntimeDependencies) -> None:
        self._deps = dependencies

    async def _maybe_await(self, value: object) -> object:
        if asyncio.iscoroutine(value) or asyncio.isfuture(value):
            return await value  # type: ignore[no-any-return]
        return value

    def _next_flow_id(self, *, prefix: str) -> str:
        try:
            flow_id = self._deps.next_raid_observability_flow_id(prefix)
        except Exception:
            flow_id = ""
        return str(flow_id or "").strip() or f"{prefix}-{int(self._deps.now() * 1000)}"

    def _log_event(
        self,
        *,
        flow_id: str,
        step: str,
        decision: str,
        level: int = logging.INFO,
        from_broadcaster_login: str | None = None,
        from_broadcaster_id: str | None = None,
        to_broadcaster_login: str | None = None,
        to_broadcaster_id: str | None = None,
        details: dict[str, Any] | None = None,
    ) -> None:
        try:
            self._deps.log_raid_observability_event(
                raid_flow_id=flow_id,
                step=step,
                decision=decision,
                level=level,
                from_broadcaster_login=from_broadcaster_login,
                from_broadcaster_id=from_broadcaster_id,
                to_broadcaster_login=to_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
                details=details or {},
            )
        except Exception:
            self._deps.logger.debug("Raid arrival observability event failed", exc_info=True)

    def _increment_counter(self, name: str, amount: int = 1) -> None:
        try:
            self._deps.increment_raid_observability_counter(name, amount)
        except Exception:
            self._deps.logger.debug("Raid arrival counter update failed: %s", name, exc_info=True)

    def _handle_secondary_confirmed_signal(
        self,
        *,
        signal_type: str,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        viewer_count: int,
        unraid_seen: bool = False,
    ) -> bool:
        recent_arrival = self._deps.lookup_recent_raid_arrival(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )
        if not recent_arrival:
            return False

        confirmation_signals = set(recent_arrival.get("confirmation_signals") or set())
        confirmation_signals.add(signal_type)
        recent_arrival["confirmation_signals"] = confirmation_signals
        recent_arrival["confirmed_ts"] = self._deps.now()
        recent_arrival["viewer_count"] = max(
            int(recent_arrival.get("viewer_count") or 0),
            int(viewer_count or 0),
        )
        arrival_tracking_id = int(recent_arrival.get("arrival_tracking_id") or 0) or None
        if arrival_tracking_id is not None:
            try:
                self._deps.update_partner_raid_arrival(
                    arrival_tracking_id,
                    confirmation_signals,
                    unraid_seen,
                )
            except Exception:
                self._deps.logger.debug(
                    "Raid arrival secondary update failed for %s -> %s",
                    from_broadcaster_login,
                    to_broadcaster_login,
                    exc_info=True,
                )

        raid_flow_id = str(recent_arrival.get("raid_flow_id") or "").strip() or self._next_flow_id(
            prefix="raid-secondary"
        )
        self._log_event(
            flow_id=raid_flow_id,
            step="secondary_signal",
            decision=signal_type,
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_login=to_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            details={
                "confirmation_signals": sorted(confirmation_signals),
                "unraid_seen": unraid_seen,
            },
        )

        self._deps.logger.info(
            "Raid arrival secondary signal recorded: %s -> %s via %s (signals=%s)",
            from_broadcaster_login,
            to_broadcaster_login,
            signal_type,
            ",".join(sorted(confirmation_signals)),
        )
        return True

    async def _execute_signal_plan_actions(self, actions: tuple[RaidSignalAction, ...]) -> None:
        for action in actions:
            kind = str(action.kind or "").strip()
            data = dict(action.data or {})
            if kind == "record_secondary_signal":
                self._handle_secondary_confirmed_signal(
                    signal_type=str(data.get("signal_type") or ""),
                    to_broadcaster_id=str(data.get("to_broadcaster_id") or ""),
                    to_broadcaster_login=str(data.get("to_broadcaster_login") or ""),
                    from_broadcaster_login=str(data.get("from_broadcaster_login") or ""),
                    viewer_count=int(data.get("viewer_count") or 0),
                    unraid_seen=bool(data.get("unraid_seen")),
                )
            elif kind == "record_pending_observation":
                pending = data.get("pending_raid")
                if isinstance(pending, PendingRaid):
                    self._deps.record_pending_signal_observation(
                        pending,
                        str(data.get("signal_type") or ""),
                        str(data.get("status") or ""),
                        str(data.get("reason") or "") or None,
                        str(data.get("detail") or "") or None,
                    )
            elif kind == "store_pending_raid":
                pending = data.get("pending_raid")
                if isinstance(pending, PendingRaid):
                    self._deps.store_pending_raid(pending)
            elif kind == "store_orphan_chat_notification":
                payload = data.get("payload")
                if isinstance(payload, dict):
                    stored_payload = dict(payload)
                    stored_payload["observed_ts"] = self._deps.now()
                    self._deps.store_orphan_chat_raid_notification(stored_payload)
            elif kind == "confirm_pending_raid":
                await self.confirm_pending_raid_arrival(
                    signal_type=str(data.get("signal_type") or ""),
                    to_broadcaster_id=str(data.get("to_broadcaster_id") or ""),
                    to_broadcaster_login=str(data.get("to_broadcaster_login") or ""),
                    from_broadcaster_login=str(data.get("from_broadcaster_login") or ""),
                    viewer_count=int(data.get("viewer_count") or 0),
                    from_broadcaster_id=str(data.get("from_broadcaster_id") or "") or None,
                )
            elif kind == "mark_manual_raid_started":
                source_key = str(data.get("source_key") or "").strip()
                if source_key:
                    self._deps.mark_manual_raid_started(
                        source_key,
                        float(data.get("ttl_seconds") or 180.0),
                    )
            elif kind == "record_independent_raid_arrival":
                self._deps.process_independent_partner_raid_arrival(
                    to_broadcaster_id=str(data.get("to_broadcaster_id") or ""),
                    to_broadcaster_login=str(data.get("to_broadcaster_login") or ""),
                    from_broadcaster_login=str(data.get("from_broadcaster_login") or ""),
                    from_broadcaster_id=str(data.get("from_broadcaster_id") or "") or None,
                    viewer_count=int(data.get("viewer_count") or 0),
                    signal_type=str(data.get("signal_type") or ""),
                    correlation_status="independent_channel_raid",
                    correlation_detail=None,
                )

    async def confirm_pending_raid_arrival(
        self,
        *,
        signal_type: str,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        viewer_count: int,
        from_broadcaster_id: str | None = None,
    ) -> None:
        pending = self._deps.pop_pending_raid(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )
        if pending is None:
            return

        decision = self._deps.arrival_confirmation_service.confirm_pending_raid_arrival(
            pending_raid=pending,
            signal_type=signal_type,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=from_broadcaster_login,
            viewer_count=viewer_count,
            from_broadcaster_id=from_broadcaster_id,
        )
        if decision is None:
            return

        pending = decision.pending_raid
        raid_flow_id = pending.raid_flow_id or self._next_flow_id(prefix="raid-arrival")
        target_stream_data = pending.target_stream_data
        registered_ts = float(pending.registered_ts or self._deps.now())
        offline_trigger_ts = pending.offline_trigger_ts
        effective_viewer_count = int(viewer_count or pending.registered_viewer_count or 0)

        if decision.should_delete_external_recruitment_blacklist_pending:
            await self._maybe_await(
                self._deps.delete_external_recruitment_blacklist_pending(to_broadcaster_id)
            )

        raid_history_id = None
        raid_history_executed_at = None
        if decision.should_load_recent_raid_history_reference:
            raid_history_id, raid_history_executed_at = self._deps.load_recent_raid_history_reference(
                from_broadcaster_login=from_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
            )

        self._deps.logger.info(
            "Raid arrival confirmed via %s: %s -> %s (%d viewers, partner_raid=%s, classification=%s, api->arrival=%.0fs, offline->arrival=%.0fs)",
            signal_type,
            from_broadcaster_login,
            to_broadcaster_login,
            effective_viewer_count,
            pending.is_partner_raid,
            decision.classification or "non_partner_target",
            self._deps.now() - registered_ts,
            (time.monotonic() - offline_trigger_ts) if offline_trigger_ts else -1.0,
        )

        arrival_tracking_id = None
        if decision.target_is_partner:
            arrival_tracking_id = await self._maybe_await(
                self._deps.store_partner_raid_arrival(
                    from_broadcaster_id=from_broadcaster_id,
                    from_broadcaster_login=from_broadcaster_login,
                    to_broadcaster_id=to_broadcaster_id,
                    to_broadcaster_login=to_broadcaster_login,
                    viewer_count=effective_viewer_count,
                    classification=decision.classification,
                    confirmation_signals={signal_type},
                    primary_signal=signal_type,
                    correlation_status="matched_pending",
                    correlation_detail=None,
                    source_resolution=decision.source_resolution,
                    raid_history_id=raid_history_id,
                    raid_history_executed_at=raid_history_executed_at,
                )
            )

        self._deps.remember_recent_raid_arrival(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            viewer_count=effective_viewer_count,
            classification=decision.classification,
            confirmation_signals={signal_type},
            arrival_tracking_id=arrival_tracking_id,
            raid_flow_id=raid_flow_id,
        )
        self._increment_counter(f"raid_arrival_confirmed_{signal_type.replace('.', '_')}_total")
        self._log_event(
            flow_id=raid_flow_id,
            step="arrival_confirmed",
            decision=signal_type,
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            details={
                "classification": decision.classification,
                "source_resolution": decision.source_resolution,
                "viewer_count": effective_viewer_count,
            },
        )

        confirmed_external_raid_count = None
        if decision.should_persist_confirmed_external_recruitment_raid:
            confirmed_external_raid_count = self._deps.record_confirmed_external_recruitment_raid(
                raid_flow_id=raid_flow_id,
                from_broadcaster_id=from_broadcaster_id,
                from_broadcaster_login=from_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
                to_broadcaster_login=to_broadcaster_login,
                viewer_count=effective_viewer_count,
                confirmation_signal=signal_type,
            )
            if confirmed_external_raid_count is None:
                self._deps.logger.warning(
                    "Confirmed external recruitment raid could not be persisted for %s (%s); skipping external follow-up",
                    to_broadcaster_login,
                    to_broadcaster_id,
                )
                return
            if decision.should_schedule_external_recruitment_blacklist_pending:
                await self._maybe_await(
                    self._deps.maybe_schedule_external_recruitment_blacklist_pending(
                        target_id=to_broadcaster_id,
                        target_login=to_broadcaster_login,
                        confirmed_raid_count=confirmed_external_raid_count,
                        raid_flow_id=raid_flow_id,
                    )
                )

        if pending.is_partner_raid and not decision.target_is_partner:
            self._deps.logger.warning(
                "Partner raid follow-up suppressed because target is no longer classified as partner: %s -> %s (signal=%s, source_resolution=%s)",
                from_broadcaster_login,
                to_broadcaster_login,
                signal_type,
                decision.source_resolution,
            )

        if decision.should_refresh_partner_score_cache:
            await self._deps.refresh_partner_score_cache_if_available(
                to_broadcaster_id,
                reason="incoming_partner_raid_confirmed",
            )
            if decision.should_track_confirmed_partner_raid and self._deps.track_confirmed_partner_raid:
                await self._maybe_await(
                    self._deps.track_confirmed_partner_raid(
                        to_broadcaster_id=to_broadcaster_id,
                        to_broadcaster_login=to_broadcaster_login,
                        from_broadcaster_login=from_broadcaster_login,
                        from_broadcaster_id=from_broadcaster_id,
                        viewer_count=effective_viewer_count,
                        score_snapshot=(
                            target_stream_data.get("_partner_score")
                            if isinstance(target_stream_data, dict)
                            else None
                        ),
                    )
                )

        if self._deps.lookup_silent_raid_enabled(to_broadcaster_login):
            self._deps.logger.info(
                "Raid message suppressed (silent_raid): %s -> %s",
                from_broadcaster_login,
                to_broadcaster_login,
            )
            return

        if decision.should_send_partner_raid_message:
            await self._deps.send_partner_raid_message(
                from_broadcaster_login=from_broadcaster_login,
                to_broadcaster_login=to_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
                viewer_count=effective_viewer_count,
            )
        elif decision.should_send_recruitment_message:
            await self._deps.send_recruitment_message(
                from_broadcaster_login=from_broadcaster_login,
                to_broadcaster_login=to_broadcaster_login,
                target_stream_data=target_stream_data,
                confirmed_external_raid_count=confirmed_external_raid_count,
            )

    async def on_raid_arrival(
        self,
        *,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        viewer_count: int,
        from_broadcaster_id: str | None = None,
    ) -> None:
        normalized_from_login = normalize_broadcaster_login(from_broadcaster_login)
        pending = self._deps.get_pending_raid(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=normalized_from_login,
        )
        if self._handle_secondary_confirmed_signal(
            signal_type="channel.raid",
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=normalized_from_login,
            viewer_count=viewer_count,
        ):
            return

        from_broadcaster_key = str(from_broadcaster_id or "").strip()
        if not from_broadcaster_key:
            from_broadcaster_key = self._deps.resolve_streamer_id_by_login(normalized_from_login) or ""

        independent_manual_detected = False
        if pending is None:
            independent_manual_detected = self._deps.process_independent_partner_raid_arrival(
                to_broadcaster_id=to_broadcaster_id,
                to_broadcaster_login=to_broadcaster_login,
                from_broadcaster_login=normalized_from_login,
                from_broadcaster_id=from_broadcaster_id,
                viewer_count=viewer_count,
                signal_type="channel.raid",
                correlation_status="independent_channel_raid",
                correlation_detail=None,
            )

        plan = self._deps.signal_correlation_service.plan_raid_arrival(
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=normalized_from_login,
            from_broadcaster_id=from_broadcaster_id,
            viewer_count=viewer_count,
            pending_raid=pending,
            recent_arrival_present=False,
            independent_manual_detected=independent_manual_detected,
            manual_raid_source_key=from_broadcaster_key or None,
        )

        if plan.outcome == "pending_mismatch" and pending is not None:
            await self._execute_signal_plan_actions(plan.actions)
            self._deps.logger.warning(
                "Raid arrival mismatch: expected from %s, got from %s",
                pending.from_broadcaster_login,
                normalized_from_login,
            )
            self._log_event(
                flow_id=pending.raid_flow_id or self._next_flow_id(prefix="raid-mismatch"),
                step="arrival_mismatch",
                decision="ignored",
                level=logging.WARNING,
                from_broadcaster_login=normalized_from_login,
                to_broadcaster_login=to_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
                details={"expected_from": pending.from_broadcaster_login},
            )
            return

        if plan.outcome == "pending_matched":
            await self._execute_signal_plan_actions(plan.actions)
            return

        if plan.outcome == "independent_manual_arrival":
            if from_broadcaster_key:
                self._deps.logger.info(
                    "External/manual raid detected via EventSub: %s -> %s. Suppressing next offline auto-raid for broadcaster_id=%s (ttl=180s/3min)",
                    normalized_from_login,
                    to_broadcaster_login,
                    from_broadcaster_key,
                )
            self._deps.logger.debug(
                "Raid arrival ignored (not pending): %s -> %s",
                normalized_from_login,
                to_broadcaster_login,
            )
            self._log_event(
                flow_id=self._next_flow_id(prefix="raid-independent"),
                step="arrival_no_pending",
                decision="ignored_or_independent",
                from_broadcaster_login=normalized_from_login,
                from_broadcaster_id=from_broadcaster_id,
                to_broadcaster_login=to_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
            )
            return

        self._deps.logger.debug(
            "Raid arrival ignored (not pending): %s -> %s",
            normalized_from_login,
            to_broadcaster_login,
        )
        self._log_event(
            flow_id=self._next_flow_id(prefix="raid-independent"),
            step="arrival_no_pending",
            decision="ignored_or_independent",
            from_broadcaster_login=normalized_from_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
        )

    async def on_chat_raid_notification(
        self,
        *,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        viewer_count: int,
        from_broadcaster_id: str | None = None,
        message_id: str | None = None,
        event_timestamp: str | None = None,
    ) -> None:
        normalized_from_login = normalize_broadcaster_login(from_broadcaster_login)
        pending = self._deps.get_pending_raid(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=normalized_from_login,
        )
        if self._handle_secondary_confirmed_signal(
            signal_type="channel.chat.notification",
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=normalized_from_login,
            viewer_count=viewer_count,
        ):
            return

        plan = self._deps.signal_correlation_service.plan_chat_notification(
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=normalized_from_login,
            from_broadcaster_id=from_broadcaster_id,
            viewer_count=viewer_count,
            message_id=message_id,
            event_timestamp=event_timestamp,
            pending_raid=pending,
            recent_arrival_present=False,
        )

        if plan.outcome == "orphan_chat_notification":
            await self._execute_signal_plan_actions(plan.actions)
            self._increment_counter("raid_orphan_chat_notification_total")
            self._deps.logger.info(
                "Orphan channel.chat.notification raid observed: %s -> %s (viewer_count=%d, grace=%.0fs, message_id=%s)",
                normalized_from_login,
                to_broadcaster_login,
                viewer_count,
                _PENDING_CHAT_NOTIFICATION_GRACE_SECONDS,
                message_id or "n/a",
            )
            self._log_event(
                flow_id=self._next_flow_id(prefix="raid-orphan"),
                step="chat_notification_orphaned",
                decision="stored",
                from_broadcaster_login=normalized_from_login,
                from_broadcaster_id=from_broadcaster_id,
                to_broadcaster_login=to_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
                details={"viewer_count": viewer_count, "message_id": message_id},
            )
            return

        if plan.outcome == "pending_mismatch" and pending is not None:
            await self._execute_signal_plan_actions(plan.actions)
            self._deps.logger.warning(
                "Raid chat notification mismatch: expected from %s, got from %s",
                pending.from_broadcaster_login,
                normalized_from_login,
            )
            self._log_event(
                flow_id=pending.raid_flow_id or self._next_flow_id(prefix="raid-chat-mismatch"),
                step="chat_notification_mismatch",
                decision="ignored",
                level=logging.WARNING,
                from_broadcaster_login=normalized_from_login,
                to_broadcaster_login=to_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
                details={"expected_from": pending.from_broadcaster_login, "message_id": message_id},
            )
            return

        if plan.outcome == "pending_matched":
            await self._execute_signal_plan_actions(plan.actions)

    async def on_chat_unraid_notification(
        self,
        *,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None = None,
        event_timestamp: str | None = None,
    ) -> None:
        normalized_from_login = normalize_broadcaster_login(from_broadcaster_login)
        pending = self._deps.get_pending_raid(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=normalized_from_login,
        )
        if self._handle_secondary_confirmed_signal(
            signal_type="channel.chat.notification.unraid",
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=normalized_from_login,
            viewer_count=0,
            unraid_seen=True,
        ):
            self._deps.logger.info(
                "channel.chat.notification unraid observed after confirmed raid: %s -> %s",
                normalized_from_login,
                to_broadcaster_login,
            )
            return

        plan = self._deps.signal_correlation_service.plan_chat_unraid(
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=normalized_from_login,
            from_broadcaster_id=from_broadcaster_id,
            pending_raid=pending,
            recent_arrival_present=False,
            event_timestamp=event_timestamp,
        )
        if plan.outcome == "pending_unraid_observed" and pending is not None:
            await self._execute_signal_plan_actions(plan.actions)

        self._deps.logger.info(
            "channel.chat.notification unraid observed without confirmed raid correlation: %s -> %s",
            normalized_from_login,
            to_broadcaster_login,
        )

    async def on_source_self_unraid_notification(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
        message_id: str | None = None,
        event_timestamp: str | None = None,
    ) -> None:
        canceled = self._deps.cancel_pending_raids_for_source_unraid(
            from_broadcaster_login=broadcaster_login,
            from_broadcaster_id=broadcaster_id,
            message_id=message_id,
            event_timestamp=event_timestamp,
        )
        if canceled > 0:
            return

        self._deps.logger.info(
            "Source self-unraid observed without pending auto-raid: %s (message_id=%s)",
            normalize_broadcaster_login(broadcaster_login),
            message_id or "n/a",
        )


__all__ = [
    "RaidArrivalRuntime",
    "RaidArrivalRuntimeDependencies",
]
