from __future__ import annotations

import logging
from collections.abc import Mapping
from typing import Any

from .pending_raids import PendingRaid, PendingRaidStore
from .raid_state_store import RaidStateStore


log = logging.getLogger("TwitchStreams.RaidManager")


class RaidTrackingArrivalFacadeMixin:
    def _ensure_runtime_raid_tracking_state(self) -> None:
        self._raid_state_store().ensure_runtime_raid_tracking_state()

    def _cleanup_stale_raid_readiness_states(self) -> None:
        self._raid_state_store().cleanup_stale_raid_readiness_states()

    @staticmethod
    def _format_pending_raid_key_for_log(key: object) -> str:
        return RaidStateStore.format_pending_raid_key_for_log(key)

    def _build_pending_raid_storage_key(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str,
    ) -> tuple[str, str]:
        return self._raid_state_store().build_pending_raid_storage_key(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )

    def _pending_raid_store(self) -> PendingRaidStore:
        return self._raid_state_store().pending_raid_store()

    def _store_pending_raid(
        self,
        pending_record: PendingRaid | Mapping[str, Any] | tuple[Any, ...],
    ) -> PendingRaid | None:
        return self._raid_state_store().store_pending_raid(pending_record)

    def _get_pending_raid(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str | None = None,
    ) -> PendingRaid | None:
        return self._raid_state_store().get_pending_raid(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )

    def _pop_pending_raid(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str | None = None,
    ) -> PendingRaid | None:
        return self._raid_state_store().pop_pending_raid(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )

    def _coerce_pending_raid_record(
        self,
        pending: PendingRaid | Mapping[str, Any] | tuple[Any, ...] | None,
        *,
        to_broadcaster_id: str | None = None,
    ) -> PendingRaid | None:
        return self._raid_state_store().coerce_pending_raid_record(
            pending,
            to_broadcaster_id=to_broadcaster_id,
        )

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
        return self._raid_tracking_runtime_service()._build_pending_raid_record(
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            target_stream_data=target_stream_data,
            is_partner_raid=is_partner_raid,
            viewer_count=viewer_count,
            offline_trigger_ts=offline_trigger_ts,
            raid_flow_id=raid_flow_id,
            channel_raid_ready=channel_raid_ready,
            channel_raid_ready_detail=channel_raid_ready_detail,
            chat_notification_state=chat_notification_state,
            chat_notification_detail=chat_notification_detail,
        )

    @staticmethod
    def _record_pending_signal_observation(
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

    def _snapshot_chat_notification_subscription(
        self,
        broadcaster_login: str,
    ) -> tuple[str | None, str | None]:
        chat_bot = getattr(self, "chat_bot", None)
        if chat_bot is None:
            return "no_chat_bot", "chat bot unavailable"

        get_state = getattr(chat_bot, "get_channel_subscription_state", None)
        if callable(get_state):
            try:
                state = get_state(broadcaster_login)
            except Exception:
                log.debug(
                    "Could not resolve channel.chat.notification subscription state for %s",
                    broadcaster_login,
                    exc_info=True,
                )
            else:
                notification_state = state.get("channel.chat.notification")
                if isinstance(notification_state, dict):
                    return (
                        str(notification_state.get("state") or "").strip() or None,
                        str(notification_state.get("detail") or "").strip() or None,
                    )

        is_ready = getattr(chat_bot, "is_channel_subscription_ready", None)
        if callable(is_ready):
            try:
                if is_ready(broadcaster_login, "channel.chat.notification"):
                    return "subscribed", None
            except Exception:
                log.debug(
                    "Could not check channel.chat.notification readiness for %s",
                    broadcaster_login,
                    exc_info=True,
                )

        return "not_joined", "channel.chat.notification not subscribed"

    def _lookup_recent_raid_arrival(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str,
    ) -> dict[str, Any] | None:
        return self._raid_state_store().lookup_recent_raid_arrival(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )

    def _remember_recent_raid_arrival(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None,
        to_broadcaster_login: str,
        viewer_count: int,
        classification: str | None,
        confirmation_signals: set[str],
        arrival_tracking_id: int | None,
        raid_flow_id: str | None = None,
    ) -> None:
        self._raid_state_store().remember_recent_raid_arrival(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            viewer_count=viewer_count,
            classification=classification,
            confirmation_signals=confirmation_signals,
            arrival_tracking_id=arrival_tracking_id,
            raid_flow_id=raid_flow_id,
        )

    def _cleanup_recent_raid_arrivals(self) -> None:
        self._raid_state_store().cleanup_recent_raid_arrivals()

    def _store_orphan_chat_raid_notification(self, payload: dict[str, Any]) -> None:
        self._raid_state_store().store_orphan_chat_raid_notification(payload)

    def _pop_orphan_chat_raid_notification(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str,
    ) -> dict[str, Any] | None:
        return self._raid_state_store().pop_orphan_chat_raid_notification(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )

    def _promote_stale_orphan_chat_raid_notifications(self) -> None:
        self._raid_state_store().promote_stale_orphan_chat_raid_notifications(
            process_independent_partner_raid_arrival=self._process_independent_partner_raid_arrival
        )

    def _resolve_known_streamer_identity(
        self,
        *,
        broadcaster_login: str,
        broadcaster_id: str | None = None,
    ) -> dict[str, str] | None:
        return self._partner_arrival_tracking_service().resolve_known_streamer_identity(
            broadcaster_login=broadcaster_login,
            broadcaster_id=broadcaster_id,
        )

    def _is_partner_target_channel(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
        expected_partner: bool = False,
    ) -> bool:
        return self._partner_arrival_tracking_service().is_partner_target_channel(
            broadcaster_id=broadcaster_id,
            broadcaster_login=broadcaster_login,
            expected_partner=expected_partner,
        )

    def _lookup_partner_target_channel(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
    ) -> Any:
        return self._partner_arrival_tracking_service().lookup_partner_target_channel(
            broadcaster_id=broadcaster_id,
            broadcaster_login=broadcaster_login,
        )

    def _classify_partner_raid_arrival(
        self,
        *,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        expected_partner: bool = False,
    ) -> tuple[str | None, str]:
        return self._partner_arrival_tracking_service().classify_partner_raid_arrival(
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            expected_partner=expected_partner,
        )

    def _load_recent_raid_history_reference(
        self,
        *,
        from_broadcaster_login: str,
        to_broadcaster_id: str,
    ) -> tuple[int | None, str | None]:
        return self._partner_arrival_tracking_service().load_recent_raid_history_reference(
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
        )

    def _store_partner_raid_arrival(
        self,
        *,
        from_broadcaster_id: str | None,
        from_broadcaster_login: str,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        viewer_count: int,
        classification: str,
        confirmation_signals: set[str],
        primary_signal: str,
        correlation_status: str,
        correlation_detail: str | None = None,
        source_resolution: str,
        raid_history_id: int | None = None,
        raid_history_executed_at: str | None = None,
        unraid_seen: bool = False,
    ) -> int | None:
        return self._partner_arrival_tracking_service().store_partner_raid_arrival(
            from_broadcaster_id=from_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            viewer_count=viewer_count,
            classification=classification,
            confirmation_signals=confirmation_signals,
            primary_signal=primary_signal,
            correlation_status=correlation_status,
            correlation_detail=correlation_detail,
            source_resolution=source_resolution,
            raid_history_id=raid_history_id,
            raid_history_executed_at=raid_history_executed_at,
            unraid_seen=unraid_seen,
        )

    def _update_partner_raid_arrival(
        self,
        *,
        arrival_tracking_id: int,
        confirmation_signals: set[str],
        unraid_seen: bool = False,
    ) -> None:
        self._partner_arrival_tracking_service().update_partner_raid_arrival(
            arrival_tracking_id=arrival_tracking_id,
            confirmation_signals=confirmation_signals,
            unraid_seen=unraid_seen,
        )

    def _process_independent_partner_raid_arrival(
        self,
        *,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None,
        viewer_count: int,
        signal_type: str,
        correlation_status: str,
        correlation_detail: str | None = None,
    ) -> bool:
        result = self._partner_arrival_tracking_service().process_independent_partner_raid_arrival_result(
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            viewer_count=viewer_count,
            signal_type=signal_type,
            correlation_status=correlation_status,
            correlation_detail=correlation_detail,
        )
        if result.processed:
            log.info(
                "Partner raid arrival classified: %s -> %s (%s via %s)",
                from_broadcaster_login,
                to_broadcaster_login,
                result.classification,
                signal_type,
            )
        return result.processed

    def _cleanup_stale_pending_raids(self):
        self._raid_tracking_runtime_service().cleanup_stale_pending_raids()

    def _clear_superseded_pending_raids(
        self,
        *,
        from_broadcaster_login: str,
        current_target_id: str,
    ) -> None:
        self._raid_tracking_runtime_service().clear_superseded_pending_raids(
            from_broadcaster_login=from_broadcaster_login,
            current_target_id=current_target_id,
        )

    def _cancel_pending_raids_for_source_unraid(
        self,
        *,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None = None,
        message_id: str | None = None,
        event_timestamp: str | None = None,
    ) -> int:
        return self._raid_tracking_runtime_service().cancel_pending_raids_for_source_unraid(
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            message_id=message_id,
            event_timestamp=event_timestamp,
        )

    async def _ensure_raid_arrival_subscription_ready(
        self,
        *,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        raid_flow_id: str | None = None,
    ) -> bool:
        self._ensure_runtime_raid_tracking_state()
        return await self._raid_tracking_runtime_service().ensure_raid_arrival_subscription_ready(
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            raid_flow_id=raid_flow_id,
        )

    async def _register_pending_raid(
        self,
        from_broadcaster_login: str,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        target_stream_data: dict | None = None,
        is_partner_raid: bool = False,
        viewer_count: int = 0,
        offline_trigger_ts: float | None = None,
        raid_flow_id: str | None = None,
        channel_raid_ready: bool | None = None,
    ):
        self._ensure_runtime_raid_tracking_state()
        await self._raid_tracking_runtime_service().register_pending_raid(
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            target_stream_data=target_stream_data,
            is_partner_raid=is_partner_raid,
            viewer_count=viewer_count,
            offline_trigger_ts=offline_trigger_ts,
            raid_flow_id=raid_flow_id,
            channel_raid_ready=channel_raid_ready,
        )

    def _build_pending_timeout_detail(self, pending_record: PendingRaid) -> str:
        return self._raid_tracking_runtime_service().build_pending_timeout_detail(
            pending_record
        )

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
        return self._raid_arrival_runtime()._handle_secondary_confirmed_signal(
            signal_type=signal_type,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=from_broadcaster_login,
            viewer_count=viewer_count,
            unraid_seen=unraid_seen,
        )

    async def _execute_signal_plan_actions(self, actions: tuple[object, ...]) -> None:
        await self._raid_arrival_runtime()._execute_signal_plan_actions(actions)

    async def _confirm_pending_raid_arrival(
        self,
        *,
        signal_type: str,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        viewer_count: int,
        from_broadcaster_id: str | None = None,
    ) -> None:
        await self._raid_arrival_runtime().confirm_pending_raid_arrival(
            signal_type=signal_type,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=from_broadcaster_login,
            viewer_count=viewer_count,
            from_broadcaster_id=from_broadcaster_id,
        )

    async def on_raid_arrival(
        self,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        viewer_count: int,
        from_broadcaster_id: str | None = None,
    ):
        await self._raid_arrival_runtime().on_raid_arrival(
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=from_broadcaster_login,
            viewer_count=viewer_count,
            from_broadcaster_id=from_broadcaster_id,
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
        await self._raid_arrival_runtime().on_chat_raid_notification(
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=from_broadcaster_login,
            viewer_count=viewer_count,
            from_broadcaster_id=from_broadcaster_id,
            message_id=message_id,
            event_timestamp=event_timestamp,
        )

    async def on_chat_unraid_notification(
        self,
        *,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None = None,
        event_timestamp: str | None = None,
    ) -> None:
        await self._raid_arrival_runtime().on_chat_unraid_notification(
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            event_timestamp=event_timestamp,
        )

    async def on_source_self_unraid_notification(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
        message_id: str | None = None,
        event_timestamp: str | None = None,
    ) -> None:
        await self._raid_arrival_runtime().on_source_self_unraid_notification(
            broadcaster_id=broadcaster_id,
            broadcaster_login=broadcaster_login,
            message_id=message_id,
            event_timestamp=event_timestamp,
        )
