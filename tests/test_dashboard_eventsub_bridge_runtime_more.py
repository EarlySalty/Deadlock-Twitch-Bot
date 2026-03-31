from __future__ import annotations

import asyncio
import json
import threading
import unittest
from collections.abc import Callable
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


class _BlockingBotApiClient:
    def __init__(self) -> None:
        self.calls: list[dict[str, Any]] = []
        self.entered = asyncio.Event()
        self.release = asyncio.Event()

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
        await self.release.wait()
        return {"ok": True}


async def _wait_for(predicate: Callable[[], bool], *, timeout: float = 2.0) -> None:
    deadline = asyncio.get_running_loop().time() + timeout
    while not predicate():
        if asyncio.get_running_loop().time() >= deadline:
            raise AssertionError("timed out waiting for condition")
        await asyncio.sleep(0.02)


class DashboardEventSubBridgeRuntimeMoreTests(unittest.IsolatedAsyncioTestCase):
    async def test_restart_processes_persisted_row_after_stop(self) -> None:
        current_time = {"now": 1000.0}
        store = _InMemoryBridgeStore()
        store.rows["restart-msg-1"] = {
            "message_id": "restart-msg-1",
            "sub_type": "stream.offline",
            "payload_json": json.dumps(
                {
                    "subscription": {"type": "stream.offline"},
                    "event": {"broadcaster_user_id": "42"},
                },
                separators=(",", ":"),
                sort_keys=True,
            ),
            "queued_at": 1000.0,
            "next_attempt_at": 1010.0,
            "attempt_count": 0,
            "last_error": None,
        }
        client = _RecordingBotApiClient()

        runtime1 = DashboardEventSubBridgeRuntime(
            client=client,
            store=store,
            now=lambda: current_time["now"],
        )
        await runtime1.start()
        await asyncio.sleep(0.05)
        await runtime1.stop()

        self.assertEqual(client.calls, [])
        self.assertIn("restart-msg-1", store.rows)

        runtime2 = DashboardEventSubBridgeRuntime(
            client=client,
            store=store,
            now=lambda: current_time["now"],
        )
        await runtime2.start()
        try:
            current_time["now"] = 1010.0
            runtime2._wakeup.set()
            await _wait_for(lambda: "restart-msg-1" not in store.rows)
        finally:
            await runtime2.stop()

        self.assertEqual(len(client.calls), 1)
        self.assertEqual(client.calls[0]["message_id"], "restart-msg-1")
        self.assertNotIn("restart-msg-1", store.dead_letters)

    async def test_concurrent_runtime_does_not_double_lease_inflight_row(self) -> None:
        current_time = {"now": 2000.0}
        store = _InMemoryBridgeStore()
        client1 = _BlockingBotApiClient()
        client2 = _RecordingBotApiClient()

        runtime1 = DashboardEventSubBridgeRuntime(
            client=client1,
            store=store,
            now=lambda: current_time["now"],
        )
        runtime2 = DashboardEventSubBridgeRuntime(
            client=client2,
            store=store,
            now=lambda: current_time["now"],
        )

        await runtime1.start()
        await runtime2.start()
        try:
            await runtime1.dispatch_or_enqueue(
                sub_type="channel.raid",
                message_id="concurrent-msg-1",
                payload={
                    "subscription": {
                        "type": "channel.raid",
                        "condition": {"to_broadcaster_user_id": "520300019"},
                    },
                    "event": {
                        "to_broadcaster_user_id": "520300019",
                        "from_broadcaster_user_id": "9901",
                    },
                },
            )
            await _wait_for(
                lambda: bool(client1.calls) and "concurrent-msg-1" in store.rows,
            )

            runtime2._wakeup.set()
            await asyncio.sleep(0.05)
            self.assertEqual(client2.calls, [])
            self.assertEqual(len(client1.calls), 1)

            client1.release.set()
            await _wait_for(lambda: "concurrent-msg-1" not in store.rows)
        finally:
            client1.release.set()
            await runtime2.stop()
            await runtime1.stop()

        self.assertEqual(len(client1.calls), 1)
        self.assertEqual(client2.calls, [])
        self.assertNotIn("concurrent-msg-1", store.dead_letters)

    async def test_duplicate_enqueue_while_dispatch_is_in_flight_is_rejected(self) -> None:
        current_time = {"now": 3000.0}
        store = _InMemoryBridgeStore()
        client = _BlockingBotApiClient()
        runtime = DashboardEventSubBridgeRuntime(
            client=client,
            store=store,
            now=lambda: current_time["now"],
        )

        await runtime.start()
        try:
            await runtime.dispatch_or_enqueue(
                sub_type="stream.offline",
                message_id="duplicate-msg-1",
                payload={
                    "subscription": {"type": "stream.offline"},
                    "event": {"broadcaster_user_id": "42"},
                },
            )
            await _wait_for(lambda: bool(client.calls) and "duplicate-msg-1" in store.rows)

            await runtime.dispatch_or_enqueue(
                sub_type="stream.offline",
                message_id="duplicate-msg-1",
                payload={
                    "subscription": {"type": "stream.offline"},
                    "event": {"broadcaster_user_id": "42"},
                },
            )
            self.assertEqual(len(store.rows), 1)
            self.assertEqual(len(client.calls), 1)

            client.release.set()
            await _wait_for(lambda: "duplicate-msg-1" not in store.rows)
        finally:
            client.release.set()
            await runtime.stop()

        self.assertEqual(len(client.calls), 1)
        self.assertNotIn("duplicate-msg-1", store.dead_letters)
