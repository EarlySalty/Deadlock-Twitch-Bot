from __future__ import annotations

import asyncio
import json
import logging
import threading
import time
from collections.abc import Callable
from typing import Any

from .. import storage
from .client import BotApiClientError

_OUTBOX_BATCH_SIZE = 20
_OUTBOX_LEASE_SECONDS = 30.0
_OUTBOX_IDLE_WAIT_SECONDS = 5.0
_OUTBOX_RETRY_BASE_SECONDS = 1.0
_OUTBOX_RETRY_MAX_SECONDS = 60.0
_OUTBOX_MAX_ATTEMPTS = 5
_OUTBOX_STARTUP_WAIT_SECONDS = 5.0


class _DashboardEventSubBridgeStartupPending(RuntimeError):
    """Raised when the bot is alive but has not activated EventSub dispatch yet."""


class DashboardEventSubBridgeStore:
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
                    CREATE TABLE IF NOT EXISTS twitch_eventsub_bridge_outbox (
                        message_id      TEXT PRIMARY KEY,
                        sub_type        TEXT NOT NULL,
                        payload_json    TEXT NOT NULL,
                        queued_at       DOUBLE PRECISION NOT NULL,
                        next_attempt_at DOUBLE PRECISION NOT NULL,
                        attempt_count   INTEGER NOT NULL DEFAULT 0,
                        last_error      TEXT
                    )
                    """
                )
                conn.execute(
                    """
                    CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_bridge_outbox_due
                    ON twitch_eventsub_bridge_outbox(next_attempt_at, queued_at)
                    """
                )
                conn.execute(
                    """
                    CREATE TABLE IF NOT EXISTS twitch_eventsub_bridge_dead_letter (
                        message_id        TEXT PRIMARY KEY,
                        sub_type          TEXT NOT NULL,
                        payload_json      TEXT NOT NULL,
                        queued_at         DOUBLE PRECISION NOT NULL,
                        dead_lettered_at  DOUBLE PRECISION NOT NULL,
                        attempt_count     INTEGER NOT NULL,
                        last_error        TEXT
                    )
                    """
                )
                conn.execute(
                    """
                    CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_bridge_dead_lettered_at
                    ON twitch_eventsub_bridge_dead_letter(dead_lettered_at)
                    """
                )
            self._initialized = True

    def enqueue(
        self,
        *,
        message_id: str,
        sub_type: str,
        payload: dict[str, Any],
        now: float,
    ) -> bool:
        self.ensure_initialized()
        payload_json = json.dumps(payload, separators=(",", ":"), sort_keys=True)
        with storage.transaction() as conn:
            row = conn.execute(
                """
                INSERT INTO twitch_eventsub_bridge_outbox (
                    message_id,
                    sub_type,
                    payload_json,
                    queued_at,
                    next_attempt_at,
                    attempt_count,
                    last_error
                )
                VALUES (%s, %s, %s, %s, %s, 0, NULL)
                ON CONFLICT (message_id) DO NOTHING
                RETURNING message_id
                """,
                (
                    message_id,
                    sub_type,
                    payload_json,
                    float(now),
                    float(now),
                ),
            ).fetchone()
        return bool(row)

    def lease_due(
        self,
        *,
        now: float,
        lease_seconds: float,
        limit: int,
    ) -> list[dict[str, Any]]:
        self.ensure_initialized()
        with storage.transaction() as conn:
            rows = conn.execute(
                """
                WITH due AS (
                    SELECT message_id
                      FROM twitch_eventsub_bridge_outbox
                     WHERE next_attempt_at <= %s
                     ORDER BY queued_at ASC
                     LIMIT %s
                     FOR UPDATE SKIP LOCKED
                )
                UPDATE twitch_eventsub_bridge_outbox AS outbox
                   SET next_attempt_at = %s
                  FROM due
                 WHERE outbox.message_id = due.message_id
                RETURNING outbox.message_id,
                          outbox.sub_type,
                          outbox.payload_json,
                          outbox.queued_at,
                          outbox.attempt_count
                """,
                (
                    float(now),
                    max(1, int(limit)),
                    float(now) + max(1.0, float(lease_seconds)),
                ),
            ).fetchall()
        leased: list[dict[str, Any]] = []
        for row in rows or []:
            leased.append(
                {
                    "message_id": str(row["message_id"] if "message_id" in row else row[0] or ""),
                    "sub_type": str(row["sub_type"] if "sub_type" in row else row[1] or ""),
                    "payload_json": str(row["payload_json"] if "payload_json" in row else row[2] or "{}"),
                    "queued_at": float(row["queued_at"] if "queued_at" in row else row[3] or 0.0),
                    "attempt_count": int(row["attempt_count"] if "attempt_count" in row else row[4] or 0),
                }
            )
        return leased

    def mark_delivered(self, *, message_id: str) -> None:
        self.ensure_initialized()
        with storage.transaction() as conn:
            conn.execute(
                "DELETE FROM twitch_eventsub_bridge_outbox WHERE message_id = %s",
                (message_id,),
            )

    def mark_retry(
        self,
        *,
        message_id: str,
        attempt_count: int,
        error_message: str,
        next_attempt_at: float,
    ) -> None:
        self.ensure_initialized()
        with storage.transaction() as conn:
            conn.execute(
                """
                UPDATE twitch_eventsub_bridge_outbox
                   SET attempt_count = %s,
                       next_attempt_at = %s,
                       last_error = %s
                 WHERE message_id = %s
                """,
                (
                    max(1, int(attempt_count)),
                    float(next_attempt_at),
                    str(error_message or "").strip()[:500] or None,
                    message_id,
                ),
            )

    def mark_deferred(
        self,
        *,
        message_id: str,
        error_message: str,
        next_attempt_at: float,
    ) -> None:
        self.ensure_initialized()
        with storage.transaction() as conn:
            conn.execute(
                """
                UPDATE twitch_eventsub_bridge_outbox
                   SET next_attempt_at = %s,
                       last_error = %s
                 WHERE message_id = %s
                """,
                (
                    float(next_attempt_at),
                    str(error_message or "").strip()[:500] or None,
                    message_id,
                ),
            )

    def mark_dead_letter(
        self,
        *,
        message_id: str,
        sub_type: str,
        payload_json: str,
        queued_at: float,
        attempt_count: int,
        error_message: str,
        dead_lettered_at: float,
    ) -> None:
        self.ensure_initialized()
        with storage.transaction() as conn:
            conn.execute(
                """
                INSERT INTO twitch_eventsub_bridge_dead_letter (
                    message_id,
                    sub_type,
                    payload_json,
                    queued_at,
                    dead_lettered_at,
                    attempt_count,
                    last_error
                )
                VALUES (%s, %s, %s, %s, %s, %s, %s)
                ON CONFLICT (message_id) DO UPDATE
                   SET sub_type = EXCLUDED.sub_type,
                       payload_json = EXCLUDED.payload_json,
                       queued_at = EXCLUDED.queued_at,
                       dead_lettered_at = EXCLUDED.dead_lettered_at,
                       attempt_count = EXCLUDED.attempt_count,
                       last_error = EXCLUDED.last_error
                """,
                (
                    message_id,
                    sub_type,
                    payload_json,
                    float(queued_at),
                    float(dead_lettered_at),
                    max(1, int(attempt_count)),
                    str(error_message or "").strip()[:500] or None,
                ),
            )
            conn.execute(
                "DELETE FROM twitch_eventsub_bridge_outbox WHERE message_id = %s",
                (message_id,),
            )


class DashboardEventSubBridgeRuntime:
    def __init__(
        self,
        *,
        client: Any,
        logger: logging.Logger | None = None,
        store: DashboardEventSubBridgeStore | None = None,
        now: Callable[[], float] | None = None,
    ) -> None:
        self._client = client
        self._log = logger or logging.getLogger("TwitchStreams.DashboardEventSubBridge")
        self._store = store or DashboardEventSubBridgeStore()
        self._now = now or time.time
        self._wakeup = asyncio.Event()
        self._stop = asyncio.Event()
        self._task: asyncio.Task[None] | None = None

    @property
    def active(self) -> bool:
        return self._task is not None and not self._task.done()

    async def start(self) -> None:
        if self.active:
            return
        self._stop.clear()
        self._task = asyncio.create_task(
            self._run(),
            name="dashboard.eventsub.bridge",
        )

    async def stop(self) -> None:
        task = self._task
        if task is None:
            return
        self._stop.set()
        self._wakeup.set()
        try:
            await task
        finally:
            self._task = None
            self._stop.clear()

    async def dispatch_or_enqueue(
        self,
        *,
        sub_type: str,
        payload: dict[str, Any],
        message_id: str,
    ) -> None:
        normalized_message_id = str(message_id or "").strip()
        if not normalized_message_id:
            raise RuntimeError("dashboard eventsub bridge requires message_id")
        if not self.active:
            await self._dispatch_once(
                sub_type=sub_type,
                payload=payload,
                message_id=normalized_message_id,
            )
            return
        queued = await asyncio.to_thread(
            self._store.enqueue,
            message_id=normalized_message_id,
            sub_type=str(sub_type or "").strip(),
            payload=dict(payload or {}),
            now=float(self._now()),
        )
        if queued:
            self._wakeup.set()

    async def _run(self) -> None:
        while not self._stop.is_set():
            self._wakeup.clear()
            processed = await self._process_due_batch()
            if processed:
                continue
            try:
                await asyncio.wait_for(
                    self._wakeup.wait(),
                    timeout=_OUTBOX_IDLE_WAIT_SECONDS,
                )
            except asyncio.TimeoutError:
                continue

    async def _process_due_batch(self) -> bool:
        leased = await asyncio.to_thread(
            self._store.lease_due,
            now=float(self._now()),
            lease_seconds=_OUTBOX_LEASE_SECONDS,
            limit=_OUTBOX_BATCH_SIZE,
        )
        if not leased:
            return False
        for record in leased:
            if self._stop.is_set():
                return True
            message_id = str(record.get("message_id") or "").strip()
            sub_type = str(record.get("sub_type") or "").strip()
            payload_json = str(record.get("payload_json") or "{}")
            queued_at = float(record.get("queued_at") or 0.0)
            attempt_count = int(record.get("attempt_count") or 0)
            try:
                payload = json.loads(payload_json)
                if not isinstance(payload, dict):
                    raise RuntimeError("invalid outbox payload")
                await self._dispatch_once(
                    sub_type=sub_type,
                    payload=payload,
                    message_id=message_id,
                )
            except _DashboardEventSubBridgeStartupPending as exc:
                next_attempt = float(self._now()) + _OUTBOX_STARTUP_WAIT_SECONDS
                await asyncio.to_thread(
                    self._store.mark_deferred,
                    message_id=message_id,
                    error_message=str(exc),
                    next_attempt_at=next_attempt,
                )
                self._log.info(
                    "Dashboard EventSub bridge waiting for bot EventSub readiness for %s msg_id=%s: %s",
                    sub_type or "unknown",
                    message_id or "n/a",
                    exc,
                )
                continue
            except Exception as exc:
                next_attempt_count = attempt_count + 1
                if next_attempt_count >= _OUTBOX_MAX_ATTEMPTS:
                    dead_lettered_at = float(self._now())
                    await asyncio.to_thread(
                        self._store.mark_dead_letter,
                        message_id=message_id,
                        sub_type=sub_type,
                        payload_json=payload_json,
                        queued_at=queued_at,
                        attempt_count=next_attempt_count,
                        error_message=str(exc),
                        dead_lettered_at=dead_lettered_at,
                    )
                    self._log.error(
                        "Dashboard EventSub bridge dead-lettered %s msg_id=%s after %d attempts: %s",
                        sub_type or "unknown",
                        message_id or "n/a",
                        next_attempt_count,
                        exc,
                    )
                    continue
                next_attempt = float(self._now()) + self._retry_delay_seconds(next_attempt_count)
                await asyncio.to_thread(
                    self._store.mark_retry,
                    message_id=message_id,
                    attempt_count=next_attempt_count,
                    error_message=str(exc),
                    next_attempt_at=next_attempt,
                )
                self._log.warning(
                    "Dashboard EventSub bridge retry scheduled for %s msg_id=%s attempt=%d/%d: %s",
                    sub_type or "unknown",
                    message_id or "n/a",
                    next_attempt_count,
                    _OUTBOX_MAX_ATTEMPTS,
                    exc,
                )
                continue
            await asyncio.to_thread(
                self._store.mark_delivered,
                message_id=message_id,
            )
        return True

    async def _dispatch_once(
        self,
        *,
        sub_type: str,
        payload: dict[str, Any],
        message_id: str,
    ) -> None:
        try:
            response = await self._client.dispatch_eventsub_notification(
                sub_type=sub_type,
                payload=payload,
                message_id=message_id,
            )
        except BotApiClientError as exc:
            if self._is_startup_pending_error(exc):
                raise _DashboardEventSubBridgeStartupPending(str(exc)) from exc
            raise RuntimeError(str(exc) or "dashboard eventsub bridge upstream unavailable") from exc
        if isinstance(response, dict) and response.get("ok") is False:
            response_message = str(response.get("message") or "").strip()
            if self._is_startup_pending_message(response_message):
                raise _DashboardEventSubBridgeStartupPending(response_message)
            raise RuntimeError(response_message or "dashboard eventsub bridge dispatch failed")

    @staticmethod
    def _is_startup_pending_error(exc: BotApiClientError) -> bool:
        if not isinstance(exc, BotApiClientError):
            return False
        return DashboardEventSubBridgeRuntime._is_startup_pending_message(str(exc.message or exc))

    @staticmethod
    def _is_startup_pending_message(message: str) -> bool:
        normalized_message = str(message or "").strip().lower()
        return normalized_message in {
            "eventsub notification dispatch inactive",
        }

    def _retry_delay_seconds(self, attempts: int) -> float:
        scaled = _OUTBOX_RETRY_BASE_SECONDS * (2 ** max(0, int(attempts) - 1))
        return min(_OUTBOX_RETRY_MAX_SECONDS, scaled)
