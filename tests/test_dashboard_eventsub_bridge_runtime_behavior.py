from __future__ import annotations

import asyncio
import json
import threading
import unittest
from collections.abc import Callable
from typing import Any

from bot.dashboard_service.client import BotApiClientError
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


class _SequencedBotApiClient:
    def __init__(self, outcomes: list[Any]) -> None:
        self._outcomes = list(outcomes)
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
        if not self._outcomes:
            raise AssertionError("unexpected extra EventSub dispatch call")
        outcome = self._outcomes.pop(0)
        if isinstance(outcome, Exception):
            raise outcome
        return outcome


async def _wait_for(predicate: Callable[[], bool], *, timeout: float = 2.5) -> None:
    deadline = asyncio.get_running_loop().time() + timeout
    while not predicate():
        if asyncio.get_running_loop().time() >= deadline:
            raise AssertionError("timed out waiting for condition")
        await asyncio.sleep(0.02)


class DashboardEventSubBridgeRuntimeBehaviorTests(unittest.IsolatedAsyncioTestCase):
    async def test_startup_pending_defers_then_recovers_with_single_final_delivery(self) -> None:
        current_time = {"now": 1000.0}
        store = _InMemoryBridgeStore()
        client = _SequencedBotApiClient(
            [
                BotApiClientError(
                    status=503,
                    code="upstream_unavailable",
                    message="eventsub notification dispatch inactive",
                ),
                {"ok": True},
            ]
        )
        runtime = DashboardEventSubBridgeRuntime(
            client=client,
            store=store,
            now=lambda: current_time["now"],
        )

        await runtime.start()
        try:
            await runtime.dispatch_or_enqueue(
                sub_type="stream.offline",
                message_id="runtime-recovery-1",
                payload={
                    "subscription": {"type": "stream.offline"},
                    "event": {"broadcaster_user_id": "42"},
                },
            )

            await _wait_for(
                lambda: len(client.calls) == 1
                and store.rows.get("runtime-recovery-1", {}).get("last_error")
                == "eventsub notification dispatch inactive",
            )
            self.assertEqual(len(client.calls), 1)
            self.assertEqual(store.rows["runtime-recovery-1"]["attempt_count"], 0)
            self.assertEqual(
                store.rows["runtime-recovery-1"]["last_error"],
                "eventsub notification dispatch inactive",
            )

            current_time["now"] = float(store.rows["runtime-recovery-1"]["next_attempt_at"])
            runtime._wakeup.set()
            await _wait_for(lambda: "runtime-recovery-1" not in store.rows)
        finally:
            await runtime.stop()

        self.assertEqual(len(client.calls), 2)
        self.assertNotIn("runtime-recovery-1", store.dead_letters)

    async def test_repeated_runtime_failure_dead_letters_without_private_batch_calls(self) -> None:
        current_time = {"now": 2000.0}
        store = _InMemoryBridgeStore()
        client = _SequencedBotApiClient(
            [
                BotApiClientError(
                    status=503,
                    code="upstream_unavailable",
                    message="temporary bridge failure",
                )
                for _ in range(5)
            ]
        )
        runtime = DashboardEventSubBridgeRuntime(
            client=client,
            store=store,
            now=lambda: current_time["now"],
        )
        runtime._retry_delay_seconds = lambda _attempts: 0.0  # type: ignore[method-assign]

        await runtime.start()
        try:
            await runtime.dispatch_or_enqueue(
                sub_type="channel.raid",
                message_id="runtime-dead-letter-1",
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

            await _wait_for(lambda: "runtime-dead-letter-1" in store.dead_letters)
        finally:
            await runtime.stop()

        self.assertNotIn("runtime-dead-letter-1", store.rows)
        self.assertEqual(len(client.calls), 5)
        self.assertEqual(store.dead_letters["runtime-dead-letter-1"]["attempt_count"], 5)
        self.assertIn(
            "temporary bridge failure",
            str(store.dead_letters["runtime-dead-letter-1"]["last_error"]),
        )
