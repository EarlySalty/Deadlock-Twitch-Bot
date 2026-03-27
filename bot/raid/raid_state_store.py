from __future__ import annotations

import logging
import time
from dataclasses import dataclass
from typing import Any, Callable, Mapping

from .pending_raids import PendingRaid, PendingRaidStore, normalize_broadcaster_login


log = logging.getLogger("TwitchStreams.RaidManager")


@dataclass(slots=True, frozen=True)
class RaidStateStoreConfig:
    recent_raid_arrival_ttl_seconds: float = 600.0
    orphan_chat_notification_grace_seconds: float = 15.0
    orphan_chat_notification_retention_seconds: float = 900.0
    raid_readiness_ttl_seconds: float = 900.0
    raid_readiness_max_entries: int = 512


class RaidStateStore:
    def __init__(
        self,
        owner: object,
        *,
        config: RaidStateStoreConfig | None = None,
        logger: logging.Logger | None = None,
        now: Callable[[], float] = time.time,
    ) -> None:
        self._owner = owner
        self._config = config or RaidStateStoreConfig()
        self._logger = logger or log
        self._now = now

    @staticmethod
    def format_pending_raid_key_for_log(key: object) -> str:
        if isinstance(key, tuple) and len(key) >= 2:
            return f"{key[0]}:{key[1]}"
        return str(key)

    @staticmethod
    def build_pending_raid_storage_key(
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str,
    ) -> tuple[str, str]:
        return (
            str(to_broadcaster_id or "").strip(),
            normalize_broadcaster_login(from_broadcaster_login),
        )

    @staticmethod
    def build_raid_arrival_cache_key(
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str,
    ) -> tuple[str, str]:
        return (
            str(to_broadcaster_id or "").strip(),
            normalize_broadcaster_login(from_broadcaster_login),
        )

    def ensure_runtime_raid_tracking_state(self) -> None:
        self._pending_raids()
        self._recent_raid_arrivals()
        self._orphan_chat_raid_notifications()
        self._raid_readiness_by_flow_id()

    def cleanup_stale_raid_readiness_states(self) -> None:
        readiness_by_flow = self._raid_readiness_by_flow_id()
        now = self._now()
        expired = [
            flow_id
            for flow_id, payload in readiness_by_flow.items()
            if now - float((payload or {}).get("checked_ts") or 0.0)
            > self._config.raid_readiness_ttl_seconds
        ]
        for flow_id in expired:
            readiness_by_flow.pop(flow_id, None)

        overflow = len(readiness_by_flow) - self._config.raid_readiness_max_entries
        if overflow <= 0:
            return

        oldest_flow_ids = sorted(
            readiness_by_flow.items(),
            key=lambda item: float((item[1] or {}).get("checked_ts") or 0.0),
        )[:overflow]
        for flow_id, _payload in oldest_flow_ids:
            readiness_by_flow.pop(flow_id, None)

    def pending_raid_store(self) -> PendingRaidStore:
        self.ensure_runtime_raid_tracking_state()
        return PendingRaidStore(self._pending_raids())

    def store_pending_raid(
        self,
        pending_record: PendingRaid | Mapping[str, Any] | tuple[Any, ...],
    ) -> PendingRaid | None:
        return self.pending_raid_store().store(pending_record)

    def get_pending_raid(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str | None = None,
    ) -> PendingRaid | None:
        return self.pending_raid_store().get(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )

    def pop_pending_raid(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str | None = None,
    ) -> PendingRaid | None:
        return self.pending_raid_store().pop(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )

    def coerce_pending_raid_record(
        self,
        pending: PendingRaid | Mapping[str, Any] | tuple[Any, ...] | None,
        *,
        to_broadcaster_id: str | None = None,
    ) -> PendingRaid | None:
        return PendingRaid.from_payload(
            pending,
            to_broadcaster_id=to_broadcaster_id,
        )

    def lookup_recent_raid_arrival(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str,
    ) -> dict[str, Any] | None:
        recent_arrivals = self._recent_raid_arrivals()
        key = self.build_raid_arrival_cache_key(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )
        arrival = recent_arrivals.get(key)
        if not arrival:
            return None
        confirmed_ts = float(arrival.get("confirmed_ts") or 0.0)
        if self._now() - confirmed_ts > self._config.recent_raid_arrival_ttl_seconds:
            recent_arrivals.pop(key, None)
            return None
        return arrival

    def remember_recent_raid_arrival(
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
        recent_arrivals = self._recent_raid_arrivals()
        key = self.build_raid_arrival_cache_key(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )
        recent_arrivals[key] = {
            "to_broadcaster_id": str(to_broadcaster_id or "").strip(),
            "to_broadcaster_login": normalize_broadcaster_login(to_broadcaster_login),
            "from_broadcaster_id": str(from_broadcaster_id or "").strip() or None,
            "from_broadcaster_login": normalize_broadcaster_login(from_broadcaster_login),
            "viewer_count": int(viewer_count or 0),
            "classification": str(classification or "").strip() or None,
            "confirmation_signals": set(confirmation_signals),
            "arrival_tracking_id": arrival_tracking_id,
            "raid_flow_id": str(raid_flow_id or "").strip() or None,
            "confirmed_ts": self._now(),
        }

    def cleanup_recent_raid_arrivals(self) -> None:
        recent_arrivals = self._recent_raid_arrivals()
        now = self._now()
        expired = [
            key
            for key, payload in recent_arrivals.items()
            if now - float(payload.get("confirmed_ts") or 0.0)
            > self._config.recent_raid_arrival_ttl_seconds
        ]
        for key in expired:
            recent_arrivals.pop(key, None)

    def store_orphan_chat_raid_notification(self, payload: dict[str, Any]) -> None:
        orphan_notifications = self._orphan_chat_raid_notifications()
        key = self.build_raid_arrival_cache_key(
            to_broadcaster_id=str(payload.get("to_broadcaster_id") or "").strip(),
            from_broadcaster_login=str(payload.get("from_broadcaster_login") or "").strip(),
        )
        payload_copy = dict(payload)
        observed_ts = payload_copy.get("observed_ts")
        payload_copy["observed_ts"] = (
            float(observed_ts) if observed_ts is not None else self._now()
        )
        orphan_notifications[key] = payload_copy

    def pop_orphan_chat_raid_notification(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str,
    ) -> dict[str, Any] | None:
        key = self.build_raid_arrival_cache_key(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )
        return self._orphan_chat_raid_notifications().pop(key, None)

    def promote_stale_orphan_chat_raid_notifications(
        self,
        *,
        process_independent_partner_raid_arrival: Callable[..., bool],
    ) -> None:
        orphan_notifications = self._orphan_chat_raid_notifications()
        now = self._now()
        stale_payloads = [
            payload
            for payload in orphan_notifications.values()
            if now - float(payload.get("observed_ts") or 0.0)
            >= self._config.orphan_chat_notification_grace_seconds
        ]
        if not stale_payloads:
            return

        for payload in stale_payloads:
            processed = process_independent_partner_raid_arrival(
                to_broadcaster_id=str(payload.get("to_broadcaster_id") or ""),
                to_broadcaster_login=str(payload.get("to_broadcaster_login") or ""),
                from_broadcaster_login=str(payload.get("from_broadcaster_login") or ""),
                from_broadcaster_id=str(payload.get("from_broadcaster_id") or "") or None,
                viewer_count=int(payload.get("viewer_count") or 0),
                signal_type="channel.chat.notification",
                correlation_status="orphan_chat_notification",
                correlation_detail="channel.chat.notification arrived before pending raid registration",
            )
            if processed:
                self.pop_orphan_chat_raid_notification(
                    to_broadcaster_id=str(payload.get("to_broadcaster_id") or ""),
                    from_broadcaster_login=str(payload.get("from_broadcaster_login") or ""),
                )
                continue

            observed_ts = float(payload.get("observed_ts") or 0.0)
            if now - observed_ts >= self._config.orphan_chat_notification_retention_seconds:
                self.pop_orphan_chat_raid_notification(
                    to_broadcaster_id=str(payload.get("to_broadcaster_id") or ""),
                    from_broadcaster_login=str(payload.get("from_broadcaster_login") or ""),
                )
                self._logger.info(
                    "Discarding stale orphan channel.chat.notification after %.0fs without correlation: %s -> %s",
                    now - observed_ts,
                    str(payload.get("from_broadcaster_login") or "").strip() or "<unknown>",
                    str(payload.get("to_broadcaster_login") or "").strip() or "<unknown>",
                )

    def _pending_raids(self) -> dict[tuple[str, str], Any]:
        return self._ensure_dict("_pending_raids")

    def _recent_raid_arrivals(self) -> dict[tuple[str, str], dict[str, Any]]:
        return self._ensure_dict("_recent_raid_arrivals")

    def _orphan_chat_raid_notifications(self) -> dict[tuple[str, str], dict[str, Any]]:
        return self._ensure_dict("_orphan_chat_raid_notifications")

    def _raid_readiness_by_flow_id(self) -> dict[str, dict[str, Any]]:
        return self._ensure_dict("_raid_readiness_by_flow_id")

    def _ensure_dict(self, attr_name: str) -> dict[Any, Any]:
        value = getattr(self._owner, attr_name, None)
        if isinstance(value, dict):
            return value
        value = {}
        setattr(self._owner, attr_name, value)
        return value


__all__ = [
    "RaidStateStore",
    "RaidStateStoreConfig",
]
