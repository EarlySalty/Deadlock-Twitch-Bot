from __future__ import annotations

import asyncio
import json
import threading
import unittest
from typing import Any

from bot.dashboard_service.eventsub_bridge import DashboardEventSubBridgeRuntime


class _InMemoryBridgeStore:
    def __init__(self) -> None:
        self._lock = threading.Lock()
        self.rows: dict[str, dict[str, Any]] = {}
        self.dead_letters: dict[str, dict[str, Any]] = {}

    def enqueue(self, *, message_id: str, sub_type: str, payload: dict[str, Any], now: float) -> bool:
        payload_json = json.dumps(payload, separators=(",", ":"), sort_keys=True)
        with self._lock:
            if message_id in self.rows or message_id in self.dead_letters:
                return False
            self.rows[message_id] = {
                "message_id": message_id,
                "sub_type": sub_type,
                "payload_json": payload_json,
                "queued_at": float(now),
                "next_attempt_at": float(now),
                "attempt_count": 0,
                "last_error": None,
            }
            return True

    def lease_due(self, *, now: float, lease_seconds: float, limit: int) -> list[dict[str, Any]]:
        del lease_seconds
        with self._lock:
            due_rows = [
                row
                for row in self.rows.values()
                if float(row.get("next_attempt_at") or 0.0) <= float(now)
            ]
            due_rows.sort(
                key=lambda row: (
                    float(row.get("queued_at") or 0.0),
                    str(row.get("message_id") or ""),
                )
            )
            leased: list[dict[str, Any]] = []
            for row in due_rows[: max(1, int(limit))]:
                leased.append(dict(row))
                row["next_attempt_at"] = float(now) + 60.0
            return leased

    def mark_delivered(self, *, message_id: str) -> None:
        with self._lock:
            self.rows.pop(message_id, None)

    def mark_retry(
        self,
        *,
        message_id: str,
        attempt_count: int,
        error_message: str,
        next_attempt_at: float,
    ) -> None:
        with self._lock:
            row = self.rows.get(message_id)
            if row is None:
                return
            row["attempt_count"] = int(attempt_count)
            row["last_error"] = error_message
            row["next_attempt_at"] = float(next_attempt_at)

    def mark_deferred(
        self,
        *,
        message_id: str,
        error_message: str,
        next_attempt_at: float,
    ) -> None:
        with self._lock:
            row = self.rows.get(message_id)
            if row is None:
                return
            row["last_error"] = error_message
            row["next_attempt_at"] = float(next_attempt_at)

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
        with self._lock:
            self.dead_letters[message_id] = {
                "message_id": message_id,
                "sub_type": sub_type,
                "payload_json": payload_json,
                "queued_at": float(queued_at),
                "dead_lettered_at": float(dead_lettered_at),
                "attempt_count": int(attempt_count),
                "last_error": error_message,
            }
            self.rows.pop(message_id, None)


class _RecordingBotApiClient:
    def __init__(self, *, outcome: dict[str, Any] | None = None) -> None:
        self.outcome = outcome or {"ok": True}
        self.calls: list[dict[str, Any]] = []

    async def dispatch_eventsub_notification(
        self,
        *,
        sub_type: str,
        payload: dict[str, Any],
        message_id: str | None = None,
    ) -> dict[str, Any]:
        self.calls.append(
            {
                "sub_type": sub_type,
                "payload": payload,
                "message_id": message_id,
            }
        )
        return dict(self.outcome)


class _SingleUseBotApiClient:
    def __init__(self) -> None:
        self.calls: list[dict[str, Any]] = []
        self.block_release = asyncio.Event()
        self.entered = asyncio.Event()

    async def dispatch_eventsub_notification(
        self,
        *,
        sub_type: str,
        payload: dict[str, Any],
        message_id: str | None = None,
    ) -> dict[str, Any]:
        self.calls.append(
            {
                "sub_type": sub_type,
                "payload": payload,
                "message_id": message_id,
            }
        )
        self.entered.set()
        await self.block_release.wait()
        return {"ok": True}


async def _wait_for(predicate, *, timeout: float = 2.0) -> None:
    deadline = asyncio.get_running_loop().time() + timeout
    while not predicate():
        if asyncio.get_running_loop().time() >= deadline:
            raise AssertionError("timed out waiting for condition")
        await asyncio.sleep(0.02)


class DashboardEventSubBridgeRuntimeEdgeTests(unittest.IsolatedAsyncioTestCase):
    async def test_start_is_idempotent_and_stop_resets_running_task(self) -> None:
        runtime = DashboardEventSubBridgeRuntime(
            client=_RecordingBotApiClient(),
            store=_InMemoryBridgeStore(),
            now=lambda: 1000.0,
        )

        await runtime.start()
        first_task = runtime._task
        await runtime.start()
        second_task = runtime._task
        await runtime.stop()

        self.assertIs(first_task, second_task)
        self.assertFalse(runtime.active)
        self.assertIsNone(runtime._task)

    async def test_dispatch_or_enqueue_before_start_dispatches_immediately_without_touching_store(self) -> None:
        store = _InMemoryBridgeStore()
        client = _RecordingBotApiClient()
        runtime = DashboardEventSubBridgeRuntime(
            client=client,
            store=store,
            now=lambda: 1000.0,
        )

        await runtime.dispatch_or_enqueue(
            sub_type="stream.offline",
            message_id="prestart-msg-1",
            payload={
                "subscription": {"type": "stream.offline"},
                "event": {"broadcaster_user_id": "42"},
            },
        )

        self.assertEqual(len(client.calls), 1)
        self.assertEqual(client.calls[0]["message_id"], "prestart-msg-1")
        self.assertEqual(store.rows, {})
        self.assertEqual(store.dead_letters, {})

    async def test_dispatch_or_enqueue_before_start_rejects_blank_message_id_without_touching_store(self) -> None:
        store = _InMemoryBridgeStore()
        client = _RecordingBotApiClient()
        runtime = DashboardEventSubBridgeRuntime(
            client=client,
            store=store,
            now=lambda: 1000.0,
        )

        with self.assertRaisesRegex(RuntimeError, "requires message_id"):
            await runtime.dispatch_or_enqueue(
                sub_type="stream.offline",
                message_id="   ",
                payload={
                    "subscription": {"type": "stream.offline"},
                    "event": {"broadcaster_user_id": "42"},
                },
            )

        self.assertEqual(client.calls, [])
        self.assertEqual(store.rows, {})
        self.assertEqual(store.dead_letters, {})

    async def test_process_due_batch_preserves_next_row_when_stop_is_requested_mid_batch(self) -> None:
        current_time = {"now": 2000.0}
        store = _InMemoryBridgeStore()
        store.rows["midbatch-1"] = {
            "message_id": "midbatch-1",
            "sub_type": "channel.raid",
            "payload_json": json.dumps(
                {
                    "subscription": {"type": "channel.raid"},
                    "event": {"to_broadcaster_user_id": "520300019"},
                },
                separators=(",", ":"),
                sort_keys=True,
            ),
            "queued_at": 2000.0,
            "next_attempt_at": 2000.0,
            "attempt_count": 0,
            "last_error": None,
        }
        store.rows["midbatch-2"] = {
            "message_id": "midbatch-2",
            "sub_type": "stream.offline",
            "payload_json": json.dumps(
                {
                    "subscription": {"type": "stream.offline"},
                    "event": {"broadcaster_user_id": "42"},
                },
                separators=(",", ":"),
                sort_keys=True,
            ),
            "queued_at": 2001.0,
            "next_attempt_at": 2000.0,
            "attempt_count": 0,
            "last_error": None,
        }
        client = _SingleUseBotApiClient()
        runtime = DashboardEventSubBridgeRuntime(
            client=client,
            store=store,
            now=lambda: current_time["now"],
        )

        await runtime.start()
        try:
            await _wait_for(lambda: client.entered.is_set())
            runtime._stop.set()
            runtime._wakeup.set()
            stop_task = asyncio.create_task(runtime.stop())
            client.block_release.set()
            await stop_task
        finally:
            client.block_release.set()
            await runtime.stop()

        self.assertEqual(len(client.calls), 1)
        self.assertNotIn("midbatch-1", store.rows)
        self.assertIn("midbatch-2", store.rows)
        self.assertEqual(store.rows["midbatch-2"]["attempt_count"], 0)
