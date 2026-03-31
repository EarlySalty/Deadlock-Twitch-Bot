from __future__ import annotations

import asyncio
import json
import logging
import threading
import time
import uuid
from collections.abc import Awaitable, Callable
from typing import Any

from .. import storage

_INBOX_BATCH_SIZE = 20
_INBOX_LEASE_SECONDS = 30.0
_INBOX_IDLE_WAIT_SECONDS = 5.0
_INBOX_RETRY_BASE_SECONDS = 1.0
_INBOX_RETRY_MAX_SECONDS = 60.0
_INBOX_MAX_ATTEMPTS = 5

EventSubProcessingHandler = Callable[[str, dict[str, Any]], Awaitable[None]]
EventSubDeadLetterHandler = Callable[[dict[str, Any]], Awaitable[None] | None]


class EventSubProcessingInboxStore:
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
                    CREATE TABLE IF NOT EXISTS twitch_eventsub_processing_inbox (
                        work_id          TEXT PRIMARY KEY,
                        work_type        TEXT NOT NULL,
                        message_id       TEXT,
                        payload_json     TEXT NOT NULL,
                        queued_at        DOUBLE PRECISION NOT NULL,
                        next_attempt_at  DOUBLE PRECISION NOT NULL,
                        attempt_count    INTEGER NOT NULL DEFAULT 0,
                        last_error       TEXT
                    )
                    """
                )
                conn.execute(
                    """
                    CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_processing_inbox_due
                    ON twitch_eventsub_processing_inbox(next_attempt_at, queued_at)
                    """
                )
                conn.execute(
                    """
                    CREATE TABLE IF NOT EXISTS twitch_eventsub_processing_dead_letter (
                        work_id           TEXT PRIMARY KEY,
                        work_type         TEXT NOT NULL,
                        message_id        TEXT,
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
                    CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_processing_dead_lettered_at
                    ON twitch_eventsub_processing_dead_letter(dead_lettered_at)
                    """
                )
            self._initialized = True

    def enqueue(
        self,
        *,
        work_type: str,
        payload: dict[str, Any],
        message_id: str | None,
        now: float,
    ) -> str:
        self.ensure_initialized()
        work_id = uuid.uuid4().hex
        payload_json = json.dumps(payload, separators=(",", ":"), sort_keys=True)
        with storage.transaction() as conn:
            conn.execute(
                """
                INSERT INTO twitch_eventsub_processing_inbox (
                    work_id,
                    work_type,
                    message_id,
                    payload_json,
                    queued_at,
                    next_attempt_at,
                    attempt_count,
                    last_error
                )
                VALUES (%s, %s, %s, %s, %s, %s, 0, NULL)
                """,
                (
                    work_id,
                    str(work_type or "").strip(),
                    str(message_id or "").strip() or None,
                    payload_json,
                    float(now),
                    float(now),
                ),
            )
        return work_id

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
                    SELECT work_id
                      FROM twitch_eventsub_processing_inbox
                     WHERE next_attempt_at <= %s
                     ORDER BY queued_at ASC
                     LIMIT %s
                     FOR UPDATE SKIP LOCKED
                )
                UPDATE twitch_eventsub_processing_inbox AS inbox
                   SET next_attempt_at = %s
                  FROM due
                 WHERE inbox.work_id = due.work_id
                RETURNING inbox.work_id,
                          inbox.work_type,
                          inbox.message_id,
                          inbox.payload_json,
                          inbox.queued_at,
                          inbox.attempt_count
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
                    "work_id": str(row["work_id"] if "work_id" in row else row[0] or ""),
                    "work_type": str(row["work_type"] if "work_type" in row else row[1] or ""),
                    "message_id": str(row["message_id"] if "message_id" in row else row[2] or ""),
                    "payload_json": str(row["payload_json"] if "payload_json" in row else row[3] or "{}"),
                    "queued_at": float(row["queued_at"] if "queued_at" in row else row[4] or 0.0),
                    "attempt_count": int(row["attempt_count"] if "attempt_count" in row else row[5] or 0),
                }
            )
        return leased

    def mark_delivered(self, *, work_id: str) -> None:
        self.ensure_initialized()
        with storage.transaction() as conn:
            conn.execute(
                "DELETE FROM twitch_eventsub_processing_inbox WHERE work_id = %s",
                (work_id,),
            )

    def mark_retry(
        self,
        *,
        work_id: str,
        attempt_count: int,
        error_message: str,
        next_attempt_at: float,
    ) -> None:
        self.ensure_initialized()
        with storage.transaction() as conn:
            conn.execute(
                """
                UPDATE twitch_eventsub_processing_inbox
                   SET attempt_count = %s,
                       next_attempt_at = %s,
                       last_error = %s
                 WHERE work_id = %s
                """,
                (
                    max(1, int(attempt_count)),
                    float(next_attempt_at),
                    str(error_message or "").strip()[:500] or None,
                    work_id,
                ),
            )

    def mark_dead_letter(
        self,
        *,
        work_id: str,
        work_type: str,
        message_id: str | None,
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
                INSERT INTO twitch_eventsub_processing_dead_letter (
                    work_id,
                    work_type,
                    message_id,
                    payload_json,
                    queued_at,
                    dead_lettered_at,
                    attempt_count,
                    last_error
                )
                VALUES (%s, %s, %s, %s, %s, %s, %s, %s)
                ON CONFLICT (work_id) DO UPDATE
                   SET work_type = EXCLUDED.work_type,
                       message_id = EXCLUDED.message_id,
                       payload_json = EXCLUDED.payload_json,
                       queued_at = EXCLUDED.queued_at,
                       dead_lettered_at = EXCLUDED.dead_lettered_at,
                       attempt_count = EXCLUDED.attempt_count,
                       last_error = EXCLUDED.last_error
                """,
                (
                    work_id,
                    work_type,
                    str(message_id or "").strip() or None,
                    payload_json,
                    float(queued_at),
                    float(dead_lettered_at),
                    max(1, int(attempt_count)),
                    str(error_message or "").strip()[:500] or None,
                ),
            )
            conn.execute(
                "DELETE FROM twitch_eventsub_processing_inbox WHERE work_id = %s",
                (work_id,),
            )

    def list_pending(self, *, limit: int = 100) -> list[dict[str, Any]]:
        self.ensure_initialized()
        with storage.readonly_connection() as conn:
            rows = conn.execute(
                """
                SELECT work_id,
                       work_type,
                       message_id,
                       payload_json,
                       queued_at,
                       next_attempt_at,
                       attempt_count,
                       last_error
                  FROM twitch_eventsub_processing_inbox
                 ORDER BY queued_at ASC
                 LIMIT %s
                """,
                (max(1, int(limit)),),
            ).fetchall()
        return [self._row_to_record(row, include_next_attempt=True) for row in rows or []]

    def list_dead_letters(self, *, limit: int = 100) -> list[dict[str, Any]]:
        self.ensure_initialized()
        with storage.readonly_connection() as conn:
            rows = conn.execute(
                """
                SELECT work_id,
                       work_type,
                       message_id,
                       payload_json,
                       queued_at,
                       dead_lettered_at,
                       attempt_count,
                       last_error
                  FROM twitch_eventsub_processing_dead_letter
                 ORDER BY dead_lettered_at DESC
                 LIMIT %s
                """,
                (max(1, int(limit)),),
            ).fetchall()
        return [self._row_to_record(row, include_dead_lettered_at=True) for row in rows or []]

    def requeue_dead_letter(self, *, work_id: str, now: float) -> bool:
        normalized_work_id = str(work_id or "").strip()
        if not normalized_work_id:
            return False
        self.ensure_initialized()
        with storage.transaction() as conn:
            row = conn.execute(
                """
                DELETE FROM twitch_eventsub_processing_dead_letter
                 WHERE work_id = %s
             RETURNING work_id,
                       work_type,
                       message_id,
                       payload_json
                """,
                (normalized_work_id,),
            ).fetchone()
            if row is None:
                return False
            conn.execute(
                """
                INSERT INTO twitch_eventsub_processing_inbox (
                    work_id,
                    work_type,
                    message_id,
                    payload_json,
                    queued_at,
                    next_attempt_at,
                    attempt_count,
                    last_error
                )
                VALUES (%s, %s, %s, %s, %s, %s, 0, NULL)
                """,
                (
                    str(row["work_id"] if "work_id" in row else row[0] or ""),
                    str(row["work_type"] if "work_type" in row else row[1] or ""),
                    str(row["message_id"] if "message_id" in row else row[2] or "") or None,
                    str(row["payload_json"] if "payload_json" in row else row[3] or "{}"),
                    float(now),
                    float(now),
                ),
            )
        return True

    @staticmethod
    def _parse_payload(payload_json: str) -> dict[str, Any]:
        try:
            parsed = json.loads(str(payload_json or "{}"))
        except Exception:
            return {}
        return parsed if isinstance(parsed, dict) else {}

    def _row_to_record(
        self,
        row: Any,
        *,
        include_next_attempt: bool = False,
        include_dead_lettered_at: bool = False,
    ) -> dict[str, Any]:
        record = {
            "work_id": str(row["work_id"] if "work_id" in row else row[0] or ""),
            "work_type": str(row["work_type"] if "work_type" in row else row[1] or ""),
            "message_id": str(row["message_id"] if "message_id" in row else row[2] or "") or None,
            "payload": self._parse_payload(str(row["payload_json"] if "payload_json" in row else row[3] or "{}")),
            "queued_at": float(row["queued_at"] if "queued_at" in row else row[4] or 0.0),
        }
        offset = 5
        if include_next_attempt:
            record["next_attempt_at"] = float(
                row["next_attempt_at"] if "next_attempt_at" in row else row[offset] or 0.0
            )
            offset += 1
        if include_dead_lettered_at:
            record["dead_lettered_at"] = float(
                row["dead_lettered_at"] if "dead_lettered_at" in row else row[offset] or 0.0
            )
            offset += 1
        record["attempt_count"] = int(
            row["attempt_count"] if "attempt_count" in row else row[offset] or 0
        )
        record["last_error"] = str(
            row["last_error"] if "last_error" in row else row[offset + 1] or ""
        ) or None
        return record


class EventSubProcessingInboxRuntime:
    def __init__(
        self,
        *,
        handler: EventSubProcessingHandler,
        on_dead_letter: EventSubDeadLetterHandler | None = None,
        logger: logging.Logger | None = None,
        store: EventSubProcessingInboxStore | None = None,
        now: Callable[[], float] | None = None,
    ) -> None:
        self._handler = handler
        self._on_dead_letter = on_dead_letter
        self._log = logger or logging.getLogger("TwitchStreams.EventSubProcessingInbox")
        self._store = store or EventSubProcessingInboxStore()
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
            name="eventsub.processing.inbox",
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

    async def enqueue(
        self,
        *,
        work_type: str,
        payload: dict[str, Any],
        message_id: str | None = None,
    ) -> str:
        work_id = await asyncio.to_thread(
            self._store.enqueue,
            work_type=str(work_type or "").strip(),
            payload=dict(payload or {}),
            message_id=str(message_id or "").strip() or None,
            now=float(self._now()),
        )
        if self.active:
            self._wakeup.set()
        return work_id

    async def snapshot(self, *, limit: int = 20) -> dict[str, Any]:
        pending = await asyncio.to_thread(
            self._store.list_pending,
            limit=max(1, int(limit)),
        )
        dead_letters = await asyncio.to_thread(
            self._store.list_dead_letters,
            limit=max(1, int(limit)),
        )
        return {
            "active": self.active,
            "pendingCount": len(pending),
            "deadLetterCount": len(dead_letters),
            "pending": pending,
            "deadLetters": dead_letters,
        }

    async def requeue_dead_letter(self, *, work_id: str) -> bool:
        requeued = await asyncio.to_thread(
            self._store.requeue_dead_letter,
            work_id=str(work_id or "").strip(),
            now=float(self._now()),
        )
        if requeued and self.active:
            self._wakeup.set()
        return requeued

    async def _run(self) -> None:
        while not self._stop.is_set():
            self._wakeup.clear()
            processed = await self._process_due_batch()
            if processed:
                continue
            try:
                await asyncio.wait_for(
                    self._wakeup.wait(),
                    timeout=_INBOX_IDLE_WAIT_SECONDS,
                )
            except asyncio.TimeoutError:
                continue

    async def _process_due_batch(self) -> bool:
        leased = await asyncio.to_thread(
            self._store.lease_due,
            now=float(self._now()),
            lease_seconds=_INBOX_LEASE_SECONDS,
            limit=_INBOX_BATCH_SIZE,
        )
        if not leased:
            return False
        for record in leased:
            if self._stop.is_set():
                return True
            work_id = str(record.get("work_id") or "").strip()
            work_type = str(record.get("work_type") or "").strip()
            message_id = str(record.get("message_id") or "").strip() or None
            payload_json = str(record.get("payload_json") or "{}")
            queued_at = float(record.get("queued_at") or 0.0)
            attempt_count = int(record.get("attempt_count") or 0)
            try:
                payload = json.loads(payload_json)
                if not isinstance(payload, dict):
                    raise RuntimeError("invalid eventsub processing payload")
                await self._handler(work_type, payload)
            except Exception as exc:
                next_attempt_count = attempt_count + 1
                if next_attempt_count >= _INBOX_MAX_ATTEMPTS:
                    dead_lettered_at = float(self._now())
                    await asyncio.to_thread(
                        self._store.mark_dead_letter,
                        work_id=work_id,
                        work_type=work_type,
                        message_id=message_id,
                        payload_json=payload_json,
                        queued_at=queued_at,
                        attempt_count=next_attempt_count,
                        error_message=str(exc),
                        dead_lettered_at=dead_lettered_at,
                    )
                    self._log.error(
                        "EventSub processing inbox dead-lettered %s work_id=%s msg_id=%s after %d attempts: %s",
                        work_type or "unknown",
                        work_id or "n/a",
                        message_id or "n/a",
                        next_attempt_count,
                        exc,
                    )
                    if callable(self._on_dead_letter):
                        result = self._on_dead_letter(
                            {
                                "work_id": work_id,
                                "work_type": work_type,
                                "message_id": message_id,
                                "payload": payload,
                                "attempt_count": next_attempt_count,
                                "last_error": str(exc),
                            }
                        )
                        if asyncio.iscoroutine(result):
                            await result
                    continue
                next_attempt = float(self._now()) + self._retry_delay_seconds(next_attempt_count)
                await asyncio.to_thread(
                    self._store.mark_retry,
                    work_id=work_id,
                    attempt_count=next_attempt_count,
                    error_message=str(exc),
                    next_attempt_at=next_attempt,
                )
                self._log.warning(
                    "EventSub processing inbox retry scheduled for %s work_id=%s msg_id=%s attempt=%d/%d: %s",
                    work_type or "unknown",
                    work_id or "n/a",
                    message_id or "n/a",
                    next_attempt_count,
                    _INBOX_MAX_ATTEMPTS,
                    exc,
                )
                continue
            await asyncio.to_thread(
                self._store.mark_delivered,
                work_id=work_id,
            )
        return True

    def _retry_delay_seconds(self, attempts: int) -> float:
        scaled = _INBOX_RETRY_BASE_SECONDS * (2 ** max(0, int(attempts) - 1))
        return min(_INBOX_RETRY_MAX_SECONDS, scaled)
