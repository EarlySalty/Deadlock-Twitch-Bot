from __future__ import annotations

import json
import unittest
from dataclasses import dataclass
from typing import Any, Callable

from bot.monitoring.eventsub_state_store import EventSubStateStore
from tests.eventsub_state_store_test_helpers import InMemoryEventSubStateRepository


@dataclass
class _InboxRow:
    message_id: str
    sub_type: str
    payload: dict[str, Any]
    queued_at: float
    next_attempt_at: float
    attempt_count: int = 0
    last_error: str | None = None


class _PersistentEventSubInboxRepository:
    def __init__(self) -> None:
        self.rows: dict[str, _InboxRow] = {}
        self.dead_letters: dict[str, dict[str, Any]] = {}

    def enqueue(self, *, message_id: str, sub_type: str, payload: dict[str, Any], now: float) -> bool:
        if message_id in self.rows or message_id in self.dead_letters:
            return False
        self.rows[message_id] = _InboxRow(
            message_id=message_id,
            sub_type=sub_type,
            payload=json.loads(json.dumps(payload)),
            queued_at=float(now),
            next_attempt_at=float(now),
        )
        return True

    def due(self, *, now: float, limit: int) -> list[_InboxRow]:
        due_rows = [
            row
            for row in self.rows.values()
            if row.next_attempt_at <= float(now)
        ]
        due_rows.sort(key=lambda row: (row.queued_at, row.message_id))
        return due_rows[: max(1, int(limit))]

    def mark_retry(self, *, message_id: str, error: str, next_attempt_at: float) -> None:
        row = self.rows[message_id]
        row.last_error = error
        row.next_attempt_at = float(next_attempt_at)

    def mark_dead_letter(self, *, message_id: str, error: str, dead_lettered_at: float) -> None:
        row = self.rows.pop(message_id)
        self.dead_letters[message_id] = {
            "message_id": row.message_id,
            "sub_type": row.sub_type,
            "payload": row.payload,
            "queued_at": row.queued_at,
            "dead_lettered_at": float(dead_lettered_at),
            "attempt_count": row.attempt_count,
            "last_error": error,
        }

    def mark_delivered(self, *, message_id: str) -> None:
        self.rows.pop(message_id, None)


class _PersistentEventSubInboxRuntime:
    def __init__(
        self,
        *,
        inbox_repo: _PersistentEventSubInboxRepository,
        guard_store: EventSubStateStore,
        now: Callable[[], float],
        max_attempts: int = 5,
    ) -> None:
        self._inbox_repo = inbox_repo
        self._guard_store = guard_store
        self._now = now
        self._max_attempts = max(1, int(max_attempts))

    def accept(self, *, message_id: str, sub_type: str, payload: dict[str, Any]) -> bool:
        if self._guard_store.is_active("message_id", message_id):
            return False
        if not self._guard_store.claim("message_id", message_id, ttl_seconds=600.0):
            return False
        queued = self._inbox_repo.enqueue(
            message_id=message_id,
            sub_type=sub_type,
            payload=payload,
            now=self._now(),
        )
        if not queued:
            self._guard_store.release("message_id", message_id)
            return False
        return True

    async def drain_due(
        self,
        *,
        callback: Callable[[str, str, dict[str, Any]], Any],
        limit: int = 20,
    ) -> None:
        for row in self._inbox_repo.due(now=self._now(), limit=limit):
            try:
                result = callback(row.message_id, row.sub_type, row.payload)
                if hasattr(result, "__await__"):
                    await result
            except Exception as exc:
                next_attempt_count = row.attempt_count + 1
                row.attempt_count = next_attempt_count
                if next_attempt_count >= self._max_attempts:
                    self._inbox_repo.mark_dead_letter(
                        message_id=row.message_id,
                        error=str(exc),
                        dead_lettered_at=self._now(),
                    )
                    self._guard_store.release("message_id", row.message_id)
                    continue
                self._inbox_repo.mark_retry(
                    message_id=row.message_id,
                    error=str(exc),
                    next_attempt_at=self._now(),
                )
                continue

            self._inbox_repo.mark_delivered(message_id=row.message_id)
            self._guard_store.release("message_id", row.message_id)


class EventSubInboxReplayLayerTests(unittest.IsolatedAsyncioTestCase):
    def _build_runtime(self) -> tuple[_PersistentEventSubInboxRepository, _PersistentEventSubInboxRuntime]:
        inbox_repo = _PersistentEventSubInboxRepository()
        guard_store = EventSubStateStore(
            repository=InMemoryEventSubStateRepository(),
        )
        runtime = _PersistentEventSubInboxRuntime(
            inbox_repo=inbox_repo,
            guard_store=guard_store,
            now=lambda: 1_000.0,
        )
        return inbox_repo, runtime

    async def test_accepted_event_is_durably_enqueued_for_deferred_processing(self) -> None:
        inbox_repo, runtime = self._build_runtime()
        accepted = runtime.accept(
            message_id="evt-1",
            sub_type="stream.offline",
            payload={"event": {"broadcaster_user_id": "1234"}},
        )

        self.assertTrue(accepted)
        self.assertIn("evt-1", inbox_repo.rows)
        self.assertEqual(inbox_repo.rows["evt-1"].attempt_count, 0)
        self.assertTrue(runtime._guard_store.is_active("message_id", "evt-1"))

    async def test_failed_deferred_processing_is_retried_from_persisted_inbox(self) -> None:
        inbox_repo, runtime = self._build_runtime()
        runtime.accept(
            message_id="evt-2",
            sub_type="stream.offline",
            payload={"event": {"broadcaster_user_id": "5678"}},
        )

        first_calls: list[str] = []

        async def _failing_callback(message_id: str, sub_type: str, payload: dict[str, Any]) -> None:
            first_calls.append(message_id)
            del sub_type, payload
            raise RuntimeError("transient failure")

        await runtime.drain_due(callback=_failing_callback)
        self.assertEqual(first_calls, ["evt-2"])
        self.assertIn("evt-2", inbox_repo.rows)
        self.assertEqual(inbox_repo.rows["evt-2"].attempt_count, 1)
        self.assertIsNotNone(inbox_repo.rows["evt-2"].last_error)

        second_calls: list[str] = []

        async def _successful_callback(message_id: str, sub_type: str, payload: dict[str, Any]) -> None:
            second_calls.append(message_id)
            del sub_type, payload
            return None

        await runtime.drain_due(callback=_successful_callback)
        self.assertEqual(second_calls, ["evt-2"])
        self.assertNotIn("evt-2", inbox_repo.rows)
        self.assertFalse(runtime._guard_store.is_active("message_id", "evt-2"))

    async def test_repeated_failure_dead_letters_after_max_attempts(self) -> None:
        inbox_repo, runtime = self._build_runtime()
        runtime.accept(
            message_id="evt-3",
            sub_type="channel.raid",
            payload={"event": {"to_broadcaster_user_id": "4321"}},
        )

        async def _always_fail(*_args, **_kwargs) -> None:
            raise RuntimeError("persistent failure")

        for _ in range(5):
            await runtime.drain_due(callback=_always_fail)

        self.assertNotIn("evt-3", inbox_repo.rows)
        self.assertIn("evt-3", inbox_repo.dead_letters)
        self.assertEqual(inbox_repo.dead_letters["evt-3"]["attempt_count"], 5)
        self.assertIn("persistent failure", inbox_repo.dead_letters["evt-3"]["last_error"])
        self.assertFalse(runtime._guard_store.is_active("message_id", "evt-3"))

    async def test_success_clears_inbox_and_guard(self) -> None:
        inbox_repo, runtime = self._build_runtime()
        runtime.accept(
            message_id="evt-4",
            sub_type="stream.online",
            payload={"event": {"broadcaster_user_id": "2468"}},
        )

        seen: list[str] = []

        async def _successful_callback(message_id: str, sub_type: str, payload: dict[str, Any]) -> None:
            seen.append(message_id)
            del sub_type, payload

        await runtime.drain_due(callback=_successful_callback)

        self.assertEqual(seen, ["evt-4"])
        self.assertNotIn("evt-4", inbox_repo.rows)
        self.assertNotIn("evt-4", inbox_repo.dead_letters)
        self.assertFalse(runtime._guard_store.is_active("message_id", "evt-4"))
