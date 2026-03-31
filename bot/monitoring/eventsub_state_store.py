from __future__ import annotations

import logging
import threading
import time
from collections.abc import Callable
from typing import Protocol

from .. import storage

EVENTSUB_STATE_KIND_MESSAGE_ID = "message_id"
EVENTSUB_STATE_KIND_WS_MESSAGE_ID = "ws_message_id"
EVENTSUB_STATE_KIND_OFFLINE_THROTTLE = "offline_throttle"
EVENTSUB_STATE_KIND_BUSINESS_EFFECT = "business_effect"


class EventSubStateRepository(Protocol):
    def ensure_initialized(self) -> None: ...

    def is_active(self, kind: str, key: str, *, now: float) -> bool: ...

    def claim(self, kind: str, key: str, *, ttl_seconds: float, now: float) -> bool: ...

    def release(self, kind: str, key: str) -> None: ...


class _PostgresEventSubStateRepository:
    def __init__(self) -> None:
        self._init_lock = threading.Lock()
        self._initialized = False

    def ensure_initialized(self) -> None:
        if self._initialized:
            return
        with self._init_lock:
            if self._initialized:
                return
            with storage.transaction() as conn:
                conn.execute(
                    """
                    CREATE TABLE IF NOT EXISTS eventsub_guard_state (
                        kind TEXT NOT NULL,
                        guard_key TEXT NOT NULL,
                        expires_at DOUBLE PRECISION NOT NULL,
                        updated_at DOUBLE PRECISION NOT NULL,
                        PRIMARY KEY (kind, guard_key)
                    )
                    """
                )
                conn.execute(
                    """
                    CREATE INDEX IF NOT EXISTS idx_eventsub_guard_state_expiry
                    ON eventsub_guard_state(expires_at)
                    """
                )
            self._initialized = True

    def is_active(self, kind: str, key: str, *, now: float) -> bool:
        with storage.readonly_connection() as conn:
            row = conn.execute(
                """
                SELECT 1
                  FROM eventsub_guard_state
                 WHERE kind = %s
                   AND guard_key = %s
                   AND expires_at > %s
                 LIMIT 1
                """,
                (kind, key, float(now)),
            ).fetchone()
        return bool(row)

    def claim(self, kind: str, key: str, *, ttl_seconds: float, now: float) -> bool:
        expires_at = float(now) + max(1.0, float(ttl_seconds))
        with storage.transaction() as conn:
            conn.execute(
                "DELETE FROM eventsub_guard_state WHERE expires_at <= %s",
                (float(now),),
            )
            row = conn.execute(
                """
                INSERT INTO eventsub_guard_state (kind, guard_key, expires_at, updated_at)
                VALUES (%s, %s, %s, %s)
                ON CONFLICT (kind, guard_key) DO UPDATE
                   SET expires_at = EXCLUDED.expires_at,
                       updated_at = EXCLUDED.updated_at
                 WHERE eventsub_guard_state.expires_at <= EXCLUDED.updated_at
                RETURNING 1
                """,
                (kind, key, expires_at, float(now)),
            ).fetchone()
        return bool(row)

    def release(self, kind: str, key: str) -> None:
        with storage.transaction() as conn:
            conn.execute(
                "DELETE FROM eventsub_guard_state WHERE kind = %s AND guard_key = %s",
                (kind, key),
            )


class EventSubStateStore:
    """Small persistent guard store shared across EventSub transports."""

    def __init__(
        self,
        *,
        logger: logging.Logger | None = None,
        repository: EventSubStateRepository | None = None,
        now: Callable[[], float] | None = None,
    ) -> None:
        self._log = logger or logging.getLogger("TwitchStreams.EventSubState")
        self._repository = repository or _PostgresEventSubStateRepository()
        self._now = now or time.time

    def _normalize(self, kind: str, key: str) -> tuple[str, str]:
        return str(kind or "").strip().lower(), str(key or "").strip()

    def is_active(self, kind: str, key: str) -> bool:
        normalized_kind, normalized_key = self._normalize(kind, key)
        if not normalized_kind or not normalized_key:
            return False
        try:
            self._repository.ensure_initialized()
            return self._repository.is_active(
                normalized_kind,
                normalized_key,
                now=float(self._now()),
            )
        except Exception:
            self._log.exception(
                "EventSub state store lookup failed for %s/%s",
                normalized_kind,
                normalized_key,
            )
            raise

    def claim(self, kind: str, key: str, *, ttl_seconds: float) -> bool:
        normalized_kind, normalized_key = self._normalize(kind, key)
        if not normalized_kind or not normalized_key:
            return False
        now = float(self._now())
        try:
            self._repository.ensure_initialized()
            return self._repository.claim(
                normalized_kind,
                normalized_key,
                ttl_seconds=ttl_seconds,
                now=now,
            )
        except Exception:
            self._log.exception(
                "EventSub state store claim failed for %s/%s",
                normalized_kind,
                normalized_key,
            )
            raise

    def release(self, kind: str, key: str) -> None:
        normalized_kind, normalized_key = self._normalize(kind, key)
        if not normalized_kind or not normalized_key:
            return
        try:
            self._repository.ensure_initialized()
            self._repository.release(normalized_kind, normalized_key)
        except Exception:
            self._log.exception(
                "EventSub state store release failed for %s/%s",
                normalized_kind,
                normalized_key,
            )
            raise
