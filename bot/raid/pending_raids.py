from __future__ import annotations

import time
from collections.abc import Iterator, Mapping, MutableMapping, Sequence
from dataclasses import dataclass, field
from typing import Any, TypeAlias

PendingRaidKey = tuple[str, str]
PendingRaidPayload: TypeAlias = "PendingRaid | Mapping[str, Any] | Sequence[Any]"


def normalize_broadcaster_login(raw_value: str | None) -> str:
    return str(raw_value or "").strip().lower()


def normalize_pending_raid_key(
    *,
    to_broadcaster_id: str | None,
    from_broadcaster_login: str | None,
) -> PendingRaidKey:
    return (
        str(to_broadcaster_id or "").strip(),
        normalize_broadcaster_login(from_broadcaster_login),
    )


def _normalize_target_stream_data(value: object) -> dict[str, Any] | None:
    if isinstance(value, Mapping):
        return dict(value)
    return None


def _normalize_signal_observations(value: object) -> dict[str, dict[str, str]]:
    if not isinstance(value, Mapping):
        return {}

    normalized: dict[str, dict[str, str]] = {}
    for signal_type, observation in value.items():
        if not isinstance(observation, Mapping):
            continue
        normalized[str(signal_type)] = {
            str(field_name): str(field_value).strip()
            for field_name, field_value in observation.items()
            if str(field_value).strip()
        }
    return normalized


def _safe_float(value: object, default: float) -> float:
    try:
        if value is None:
            return float(default)
        return float(value)
    except (TypeError, ValueError):
        return float(default)


def _safe_optional_float(value: object) -> float | None:
    try:
        if value is None:
            return None
        return float(value)
    except (TypeError, ValueError):
        return None


def _safe_int(value: object, default: int) -> int:
    try:
        if value is None:
            return int(default)
        return int(value)
    except (TypeError, ValueError):
        return int(default)


def _coerce_bool(value: object, default: bool = False) -> bool:
    if value is None:
        return bool(default)
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return bool(value)
    if isinstance(value, str):
        normalized = value.strip().lower()
        if normalized in {"", "0", "false", "no", "off", "none", "null"}:
            return False
        if normalized in {"1", "true", "yes", "on"}:
            return True
    return bool(value)


def _coerce_optional_bool(value: object) -> bool | None:
    if value is None:
        return None
    return _coerce_bool(value)


@dataclass(slots=True)
class PendingRaid:
    from_broadcaster_login: str
    to_broadcaster_id: str
    target_stream_data: dict[str, Any] | None = None
    registered_ts: float = field(default_factory=time.time)
    is_partner_raid: bool = False
    registered_viewer_count: int = 0
    offline_trigger_ts: float | None = None
    raid_flow_id: str | None = None
    channel_raid_ready: bool | None = None
    channel_raid_ready_detail: str | None = None
    chat_notification_state: str | None = None
    chat_notification_detail: str | None = None
    signal_observations: dict[str, dict[str, str]] = field(default_factory=dict)

    @property
    def key(self) -> PendingRaidKey:
        return normalize_pending_raid_key(
            to_broadcaster_id=self.to_broadcaster_id,
            from_broadcaster_login=self.from_broadcaster_login,
        )

    def normalize(self) -> "PendingRaid":
        default_registered_ts = time.time()
        self.from_broadcaster_login = normalize_broadcaster_login(
            self.from_broadcaster_login
        )
        self.to_broadcaster_id = str(self.to_broadcaster_id or "").strip()
        self.target_stream_data = _normalize_target_stream_data(self.target_stream_data)
        self.registered_ts = _safe_float(self.registered_ts, default_registered_ts)
        self.is_partner_raid = _coerce_bool(self.is_partner_raid)
        self.registered_viewer_count = _safe_int(self.registered_viewer_count, 0)
        self.offline_trigger_ts = _safe_optional_float(self.offline_trigger_ts)
        self.raid_flow_id = str(self.raid_flow_id or "").strip() or None
        self.channel_raid_ready = _coerce_optional_bool(self.channel_raid_ready)
        self.channel_raid_ready_detail = (
            str(self.channel_raid_ready_detail or "").strip() or None
        )
        self.chat_notification_state = (
            str(self.chat_notification_state or "").strip() or None
        )
        self.chat_notification_detail = (
            str(self.chat_notification_detail or "").strip() or None
        )
        self.signal_observations = _normalize_signal_observations(self.signal_observations)
        return self

    def record_signal_observation(
        self,
        *,
        signal_type: str,
        status: str,
        reason: str | None = None,
        detail: str | None = None,
    ) -> None:
        observation = {"status": str(status or "").strip()}
        if reason:
            observation["reason"] = str(reason).strip()
        if detail:
            observation["detail"] = str(detail).strip()
        self.signal_observations[str(signal_type)] = observation

    def to_dict(self) -> dict[str, Any]:
        return {
            "from_broadcaster_login": self.from_broadcaster_login,
            "to_broadcaster_id": self.to_broadcaster_id,
            "target_stream_data": (
                dict(self.target_stream_data) if isinstance(self.target_stream_data, dict) else None
            ),
            "registered_ts": float(self.registered_ts),
            "is_partner_raid": bool(self.is_partner_raid),
            "registered_viewer_count": int(self.registered_viewer_count),
            "offline_trigger_ts": self.offline_trigger_ts,
            "raid_flow_id": self.raid_flow_id,
            "channel_raid_ready": self.channel_raid_ready,
            "channel_raid_ready_detail": self.channel_raid_ready_detail,
            "chat_notification_state": self.chat_notification_state,
            "chat_notification_detail": self.chat_notification_detail,
            "signal_observations": {
                signal_type: dict(observation)
                for signal_type, observation in self.signal_observations.items()
            },
        }

    @classmethod
    def from_payload(
        cls,
        payload: PendingRaidPayload | None,
        *,
        to_broadcaster_id: str | None = None,
        from_broadcaster_login: str | None = None,
        now: float | None = None,
    ) -> "PendingRaid" | None:
        if payload is None:
            return None

        default_registered_ts = float(now if now is not None else time.time())

        if isinstance(payload, PendingRaid):
            raid = payload
            if to_broadcaster_id is not None and not raid.to_broadcaster_id:
                raid.to_broadcaster_id = str(to_broadcaster_id or "").strip()
            if from_broadcaster_login is not None and not raid.from_broadcaster_login:
                raid.from_broadcaster_login = normalize_broadcaster_login(
                    from_broadcaster_login
                )
            if not raid.registered_ts:
                raid.registered_ts = default_registered_ts
            return raid.normalize()

        if isinstance(payload, Mapping):
            raw = dict(payload)
            target_stream_data = raw.get("target_stream_data")
            signal_observations = raw.get("signal_observations")
            raid = cls(
                from_broadcaster_login=normalize_broadcaster_login(
                    raw.get("from_broadcaster_login") or from_broadcaster_login
                ),
                to_broadcaster_id=str(
                    raw.get("to_broadcaster_id") or to_broadcaster_id or ""
                ).strip(),
                target_stream_data=_normalize_target_stream_data(target_stream_data),
                registered_ts=_safe_float(
                    raw.get("registered_ts"),
                    default_registered_ts,
                ),
                is_partner_raid=_coerce_bool(raw.get("is_partner_raid")),
                registered_viewer_count=_safe_int(
                    raw.get("registered_viewer_count"),
                    0,
                ),
                offline_trigger_ts=_safe_optional_float(raw.get("offline_trigger_ts")),
                raid_flow_id=str(raw.get("raid_flow_id") or "").strip() or None,
                channel_raid_ready=_coerce_optional_bool(raw.get("channel_raid_ready")),
                channel_raid_ready_detail=str(
                    raw.get("channel_raid_ready_detail") or ""
                ).strip() or None,
                chat_notification_state=str(
                    raw.get("chat_notification_state") or ""
                ).strip() or None,
                chat_notification_detail=str(
                    raw.get("chat_notification_detail") or ""
                ).strip() or None,
                signal_observations=_normalize_signal_observations(signal_observations),
            )
            return raid.normalize()

        if isinstance(payload, Sequence) and not isinstance(
            payload, (str, bytes, bytearray)
        ):
            legacy = list(payload)
            raid = cls(
                from_broadcaster_login=normalize_broadcaster_login(
                    legacy[0] if len(legacy) > 0 else from_broadcaster_login
                ),
                to_broadcaster_id=str(to_broadcaster_id or "").strip(),
                target_stream_data=_normalize_target_stream_data(
                    legacy[1] if len(legacy) > 1 else None
                ),
                registered_ts=_safe_float(
                    legacy[2] if len(legacy) > 2 else None,
                    default_registered_ts,
                ),
                is_partner_raid=_coerce_bool(legacy[3] if len(legacy) > 3 else False),
                registered_viewer_count=_safe_int(
                    legacy[4] if len(legacy) > 4 else None,
                    0,
                ),
                offline_trigger_ts=(
                    _safe_optional_float(legacy[5]) if len(legacy) > 5 else None
                ),
            )
            return raid.normalize()

        return None


@dataclass(slots=True, frozen=True)
class PendingRaidEntry:
    key: PendingRaidKey
    raid: PendingRaid


class PendingRaidStore:
    def __init__(
        self,
        pending_raids: MutableMapping[object, object] | None = None,
    ) -> None:
        self._pending_raids: MutableMapping[object, object] = (
            pending_raids if pending_raids is not None else {}
        )

    def normalize_in_place(self) -> int:
        normalized: dict[PendingRaidKey, PendingRaid] = {}
        migrated = 0
        for raw_key, pending in list(self._pending_raids.items()):
            key, raid = self._coerce_entry(raw_key, pending)
            if raid is None:
                continue
            if raw_key != key or pending is not raid:
                migrated += 1
            normalized[key] = raid
        if normalized != self._pending_raids:
            self._pending_raids.clear()
            self._pending_raids.update(normalized)
        return migrated

    def iter_entries(self) -> Iterator[PendingRaidEntry]:
        self.normalize_in_place()
        for key, raid in self._pending_raids.items():
            yield PendingRaidEntry(key=key, raid=raid)  # type: ignore[arg-type]

    def store(
        self,
        pending: PendingRaidPayload | None,
        *,
        to_broadcaster_id: str | None = None,
        from_broadcaster_login: str | None = None,
        now: float | None = None,
    ) -> PendingRaid | None:
        raid = PendingRaid.from_payload(
            pending,
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
            now=now,
        )
        if raid is None:
            return None
        self._pending_raids[raid.key] = raid
        return raid

    def get(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str | None = None,
    ) -> PendingRaid | None:
        self.normalize_in_place()
        target_id = str(to_broadcaster_id or "").strip()
        if not target_id:
            return None

        normalized_from = normalize_broadcaster_login(from_broadcaster_login)
        if normalized_from:
            exact_key = normalize_pending_raid_key(
                to_broadcaster_id=target_id,
                from_broadcaster_login=normalized_from,
            )
            exact = self._pending_raids.get(exact_key)
            if isinstance(exact, PendingRaid):
                return exact
            if exact is not None:
                raid = PendingRaid.from_payload(
                    exact,
                    to_broadcaster_id=target_id,
                    from_broadcaster_login=normalized_from,
                )
                if raid is not None:
                    self._pending_raids[exact_key] = raid
                return raid
            return None

        matches = [
            raid
            for key, raid in self._pending_raids.items()
            if key[0] == target_id
        ]
        if len(matches) != 1:
            return None
        return matches[0]

    def pop(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str | None = None,
    ) -> PendingRaid | None:
        self.normalize_in_place()
        target_id = str(to_broadcaster_id or "").strip()
        if not target_id:
            return None

        normalized_from = normalize_broadcaster_login(from_broadcaster_login)
        if normalized_from:
            exact_key = normalize_pending_raid_key(
                to_broadcaster_id=target_id,
                from_broadcaster_login=normalized_from,
            )
            exact = self._pending_raids.pop(exact_key, None)
            if isinstance(exact, PendingRaid):
                return exact
            if exact is not None:
                return PendingRaid.from_payload(
                    exact,
                    to_broadcaster_id=target_id,
                    from_broadcaster_login=normalized_from,
                )
            return None

        matches = [
            key
            for key, raid in self._pending_raids.items()
            if key[0] == target_id
        ]
        if len(matches) != 1:
            return None
        key = matches[0]
        pending = self._pending_raids.pop(key, None)
        if isinstance(pending, PendingRaid):
            return pending
        return PendingRaid.from_payload(pending, to_broadcaster_id=target_id)

    def cleanup_stale(
        self,
        *,
        timeout_seconds: float = 300.0,
        now: float | None = None,
    ) -> list[PendingRaid]:
        self.normalize_in_place()
        current_time = float(now if now is not None else time.time())
        stale_keys = [
            key
            for key, raid in self._pending_raids.items()
            if current_time - float(raid.registered_ts) > float(timeout_seconds)
        ]
        removed: list[PendingRaid] = []
        for key in stale_keys:
            raid = self._pending_raids.pop(key, None)
            if isinstance(raid, PendingRaid):
                removed.append(raid)
        return removed

    def supersede_from_source(
        self,
        *,
        from_broadcaster_login: str,
        current_target_id: str,
    ) -> list[PendingRaid]:
        self.normalize_in_place()
        normalized_from = normalize_broadcaster_login(from_broadcaster_login)
        current_target = str(current_target_id or "").strip()
        if not normalized_from:
            return []

        removed: list[PendingRaid] = []
        for key, raid in list(self._pending_raids.items()):
            if key[0] == current_target:
                continue
            if raid.from_broadcaster_login != normalized_from:
                continue
            removed.append(self._pending_raids.pop(key))
        return [raid for raid in removed if isinstance(raid, PendingRaid)]

    def _coerce_entry(
        self,
        raw_key: object,
        pending: object,
    ) -> tuple[PendingRaidKey, PendingRaid | None]:
        fallback_target_id = ""
        fallback_from_login = ""
        if isinstance(raw_key, tuple) and len(raw_key) >= 2:
            fallback_target_id = str(raw_key[0] or "").strip()
            fallback_from_login = normalize_broadcaster_login(raw_key[1])
        else:
            fallback_target_id = str(raw_key or "").strip()

        raid = PendingRaid.from_payload(
            pending,  # type: ignore[arg-type]
            to_broadcaster_id=fallback_target_id,
            from_broadcaster_login=fallback_from_login,
        )
        if raid is None:
            return normalize_pending_raid_key(
                to_broadcaster_id=fallback_target_id,
                from_broadcaster_login=fallback_from_login,
            ), None
        return raid.key, raid


__all__ = [
    "PendingRaid",
    "PendingRaidEntry",
    "PendingRaidKey",
    "PendingRaidStore",
    "normalize_broadcaster_login",
    "normalize_pending_raid_key",
]
