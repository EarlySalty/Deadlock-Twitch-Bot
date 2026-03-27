from __future__ import annotations

import json
import time
from dataclasses import dataclass, field
from datetime import datetime
from typing import Any, Callable, Mapping


@dataclass(slots=True, frozen=True)
class RaidObservabilityEvent:
    flow_type: str
    flow_id: str
    step: str
    decision: str
    from_broadcaster_login: str | None
    from_broadcaster_id: str | None
    to_broadcaster_login: str | None
    to_broadcaster_id: str | None
    details: dict[str, object] = field(default_factory=dict)

    @property
    def entity_login(self) -> str:
        return self.to_broadcaster_login or self.from_broadcaster_login or ""

    @property
    def entity_id(self) -> str:
        return self.to_broadcaster_id or self.from_broadcaster_id or ""

    def as_log_fields(self) -> dict[str, object]:
        return {
            "raid_flow_id": self.flow_id,
            "step": self.step,
            "decision": self.decision,
            "from_broadcaster_login": self.from_broadcaster_login,
            "from_broadcaster_id": self.from_broadcaster_id,
            "to_broadcaster_login": self.to_broadcaster_login,
            "to_broadcaster_id": self.to_broadcaster_id,
            "details": self.details,
        }

    def as_storage_payload(self) -> dict[str, object]:
        return {
            "flow_type": self.flow_type,
            "flow_id": self.flow_id,
            "entity_login": self.entity_login,
            "entity_id": self.entity_id,
            "step": self.step,
            "decision": self.decision,
            "details": dict(self.details),
        }


EventSink = Callable[[RaidObservabilityEvent], Any]


@dataclass(slots=True)
class RaidObservabilityService:
    event_sink: EventSink | None = None
    time_source: Callable[[], float] = time.time
    sequence: int = 0
    counter_store: dict[str, int] = field(default_factory=dict)

    def next_flow_id(self, *, prefix: str = "raid") -> str:
        self.sequence = int(self.sequence or 0) + 1
        normalized_prefix = str(prefix or "raid").strip().lower() or "raid"
        return f"{normalized_prefix}-{int(self.time_source() * 1000)}-{self.sequence}"

    def counters(self) -> dict[str, int]:
        if not isinstance(self.counter_store, dict):
            self.counter_store = {}
        return self.counter_store

    def increment_counter(self, name: str, amount: int = 1) -> int:
        counter_name = str(name or "").strip()
        if not counter_name:
            return 0
        counters = self.counters()
        counters[counter_name] = int(counters.get(counter_name, 0) or 0) + int(amount)
        return counters[counter_name]

    @staticmethod
    def normalize_value(value: object, *, limit: int = 240) -> str:
        def _convert(obj: object) -> object:
            if obj is None:
                return None
            if isinstance(obj, datetime):
                return obj.isoformat()
            if isinstance(obj, set):
                return sorted(str(item) for item in obj)
            if isinstance(obj, (list, tuple)):
                return [_convert(item) for item in obj]
            if isinstance(obj, Mapping):
                return {str(key): _convert(val) for key, val in obj.items()}
            if isinstance(obj, (str, int, float, bool)):
                return obj
            return str(obj)

        normalized = _convert(value)
        if isinstance(normalized, str):
            text = normalized.replace("\r", " ").replace("\n", " ").strip()
        else:
            text = json.dumps(normalized, sort_keys=True, ensure_ascii=True, separators=(",", ":"))
        if len(text) > limit:
            return f"{text[:limit]}..."
        return text

    def format_fields(self, **fields: object) -> str:
        parts: list[str] = []
        for key in sorted(fields):
            value = fields[key]
            if value is None:
                continue
            parts.append(f"{str(key).strip()}={self.normalize_value(value)}")
        return " ".join(parts)

    @staticmethod
    def _normalize_login(raw_value: str | None) -> str:
        return str(raw_value or "").strip().lower()

    @staticmethod
    def _normalize_identifier(raw_value: str | None) -> str | None:
        text = str(raw_value or "").strip()
        return text or None

    def build_event_payload(
        self,
        *,
        flow_type: str,
        flow_id: str,
        step: str,
        decision: str,
        from_broadcaster_login: str | None = None,
        from_broadcaster_id: str | None = None,
        to_broadcaster_login: str | None = None,
        to_broadcaster_id: str | None = None,
        details: Mapping[str, object] | None = None,
    ) -> RaidObservabilityEvent:
        return RaidObservabilityEvent(
            flow_type=str(flow_type or "").strip() or "raid",
            flow_id=str(flow_id or "").strip(),
            step=str(step or "").strip() or "event",
            decision=str(decision or "").strip() or "unknown",
            from_broadcaster_login=self._normalize_login(from_broadcaster_login) or None,
            from_broadcaster_id=self._normalize_identifier(from_broadcaster_id),
            to_broadcaster_login=self._normalize_login(to_broadcaster_login) or None,
            to_broadcaster_id=self._normalize_identifier(to_broadcaster_id),
            details=dict(details or {}),
        )

    def emit_event(
        self,
        *,
        flow_type: str,
        flow_id: str,
        step: str,
        decision: str,
        from_broadcaster_login: str | None = None,
        from_broadcaster_id: str | None = None,
        to_broadcaster_login: str | None = None,
        to_broadcaster_id: str | None = None,
        details: Mapping[str, object] | None = None,
    ) -> RaidObservabilityEvent:
        event = self.build_event_payload(
            flow_type=flow_type,
            flow_id=flow_id,
            step=step,
            decision=decision,
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            details=details,
        )
        if self.event_sink is not None:
            self.event_sink(event)
        return event


__all__ = [
    "EventSink",
    "RaidObservabilityEvent",
    "RaidObservabilityService",
]
