from __future__ import annotations

import asyncio
import json
import unittest
from typing import Any

from bot.monitoring.eventsub_processing_inbox import EventSubProcessingInboxRuntime


class _InMemoryProcessingStore:
    def __init__(self) -> None:
        self.rows: dict[str, dict[str, Any]] = {}
        self.dead_letters: dict[str, dict[str, Any]] = {}
        self._counter = 0

    def enqueue(
        self,
        *,
        work_type: str,
        payload: dict[str, Any],
        message_id: str | None,
        now: float,
    ) -> str:
        self._counter += 1
        work_id = f"work-{self._counter}"
        self.rows[work_id] = {
            "work_id": work_id,
            "work_type": work_type,
            "message_id": message_id,
            "payload_json": json.dumps(payload),
            "queued_at": float(now),
            "next_attempt_at": float(now),
            "attempt_count": 0,
            "last_error": None,
        }
        return work_id

    def lease_due(self, *, now: float, lease_seconds: float, limit: int) -> list[dict[str, Any]]:
        leased: list[dict[str, Any]] = []
        for row in sorted(self.rows.values(), key=lambda item: (item["queued_at"], item["work_id"])):
            if len(leased) >= limit:
                break
            if float(row.get("next_attempt_at") or 0.0) > float(now):
                continue
            leased.append(dict(row))
            row["next_attempt_at"] = float(now) + max(1.0, float(lease_seconds))
        return leased

    def mark_delivered(self, *, work_id: str) -> None:
        self.rows.pop(work_id, None)

    def mark_retry(
        self,
        *,
        work_id: str,
        attempt_count: int,
        error_message: str,
        next_attempt_at: float,
    ) -> None:
        row = self.rows.get(work_id)
        if row is None:
            return
        row["attempt_count"] = int(attempt_count)
        row["last_error"] = str(error_message)
        row["next_attempt_at"] = float(next_attempt_at)

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
        self.dead_letters[work_id] = {
            "work_id": work_id,
            "work_type": work_type,
            "message_id": message_id,
            "payload_json": payload_json,
            "queued_at": float(queued_at),
            "attempt_count": int(attempt_count),
            "last_error": str(error_message),
            "dead_lettered_at": float(dead_lettered_at),
        }
        self.rows.pop(work_id, None)


class EventSubProcessingInboxRuntimeTests(unittest.IsolatedAsyncioTestCase):
    async def test_processing_inbox_retries_and_eventually_succeeds(self) -> None:
        store = _InMemoryProcessingStore()
        calls: list[str] = []

        async def _handler(work_type: str, payload: dict[str, Any]) -> None:
            calls.append(work_type)
            if len(calls) == 1:
                raise RuntimeError("transient failure")
            self.assertEqual(payload, {"broadcaster_id": "123"})

        runtime = EventSubProcessingInboxRuntime(
            handler=_handler,
            store=store,
            now=lambda: 1000.0,
        )
        runtime._retry_delay_seconds = lambda _attempts: 0.0  # type: ignore[method-assign]

        await runtime.enqueue(
            work_type="stream.offline",
            message_id="msg-1",
            payload={"broadcaster_id": "123"},
        )
        await runtime.start()
        try:
            deadline = asyncio.get_running_loop().time() + 1.0
            while store.rows:
                if asyncio.get_running_loop().time() >= deadline:
                    raise AssertionError("expected processing inbox row to be delivered")
                await asyncio.sleep(0.02)
        finally:
            await runtime.stop()

        self.assertEqual(calls, ["stream.offline", "stream.offline"])
        self.assertEqual(store.dead_letters, {})

    async def test_processing_inbox_dead_letters_after_max_attempts(self) -> None:
        store = _InMemoryProcessingStore()
        attempts: list[str] = []

        async def _handler(work_type: str, payload: dict[str, Any]) -> None:
            attempts.append(f"{work_type}:{payload['broadcaster_id']}")
            raise RuntimeError("persistent failure")

        runtime = EventSubProcessingInboxRuntime(
            handler=_handler,
            store=store,
            now=lambda: 1000.0,
        )
        runtime._retry_delay_seconds = lambda _attempts: 0.0  # type: ignore[method-assign]

        work_id = await runtime.enqueue(
            work_type="stream.online.followups",
            message_id=None,
            payload={"broadcaster_id": "999"},
        )
        await runtime.start()
        try:
            deadline = asyncio.get_running_loop().time() + 1.0
            while work_id not in store.dead_letters:
                if asyncio.get_running_loop().time() >= deadline:
                    raise AssertionError("expected processing inbox row to be dead-lettered")
                await asyncio.sleep(0.02)
        finally:
            await runtime.stop()

        self.assertEqual(len(attempts), 5)
        self.assertNotIn(work_id, store.rows)
        self.assertEqual(store.dead_letters[work_id]["attempt_count"], 5)
