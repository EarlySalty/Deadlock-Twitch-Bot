from __future__ import annotations

import logging
import time
from dataclasses import dataclass, field
from typing import Any, Callable


log = logging.getLogger("TwitchStreams.RaidManager")


@dataclass(slots=True, frozen=True)
class ManualRaidSuppressionDependencies:
    readonly_connection: Callable[[], Any]
    load_active_partner: Callable[..., Any]
    logger: logging.Logger = field(default_factory=lambda: log)
    now: Callable[[], float] = time.time


class ManualRaidSuppressionService:
    def __init__(
        self,
        owner: object,
        dependencies: ManualRaidSuppressionDependencies,
    ) -> None:
        self._owner = owner
        self._deps = dependencies

    def mark_manual_raid_started(self, broadcaster_id: str, ttl_seconds: float = 300.0) -> None:
        broadcaster_key = str(broadcaster_id or "").strip()
        if not broadcaster_key:
            return
        ttl = max(30.0, float(ttl_seconds or 0.0))
        self._store()[broadcaster_key] = self._deps.now() + ttl

    def is_offline_auto_raid_suppressed(self, broadcaster_id: str) -> bool:
        broadcaster_key = str(broadcaster_id or "").strip()
        if not broadcaster_key:
            return False
        now = self._deps.now()
        until = self._store().get(broadcaster_key)
        if until is None:
            return False
        if now <= float(until):
            return True
        self._store().pop(broadcaster_key, None)
        return False

    def resolve_streamer_id_by_login(self, broadcaster_login: str) -> str | None:
        login_key = str(broadcaster_login or "").strip().lower()
        if not login_key:
            return None
        try:
            with self._deps.readonly_connection() as conn:
                row = self._deps.load_active_partner(conn, twitch_login=login_key)
            if not row:
                return None
            resolved = row["twitch_user_id"] if hasattr(row, "keys") else row[1]
            resolved_key = str(resolved or "").strip()
            return resolved_key or None
        except Exception:
            self._deps.logger.debug(
                "Konnte broadcaster_id nicht über Login auflösen: %s",
                login_key,
                exc_info=True,
            )
            return None

    def cleanup_expired_manual_raid_suppressions(self) -> None:
        now = self._deps.now()
        store = self._store()
        expired = [
            broadcaster_id
            for broadcaster_id, until in store.items()
            if now > float(until or 0.0)
        ]
        for broadcaster_id in expired:
            store.pop(broadcaster_id, None)
        if expired:
            self._deps.logger.debug(
                "Cleaned up %d expired manual raid suppressions",
                len(expired),
            )

    def _store(self) -> dict[str, float]:
        store = getattr(self._owner, "_manual_raid_suppression", None)
        if isinstance(store, dict):
            return store
        store = {}
        setattr(self._owner, "_manual_raid_suppression", store)
        return store


__all__ = [
    "ManualRaidSuppressionDependencies",
    "ManualRaidSuppressionService",
]
