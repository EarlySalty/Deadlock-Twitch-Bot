from __future__ import annotations

import asyncio
import json
import logging
import threading
import unittest
from collections.abc import Callable
from typing import Any
from unittest.mock import patch

from bot.dashboard_service.client import BotApiClientError
from bot.dashboard_service.eventsub_bridge import DashboardEventSubBridgeRuntime
from bot.monitoring.eventsub_state_store import EventSubStateStore
from tests.eventsub_state_store_test_helpers import InMemoryEventSubStateRepository


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
            due_rows.sort(key=lambda row: (float(row.get("queued_at") or 0.0), str(row.get("message_id") or "")))
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
                "attempt_count": int(attempt_count),
                "last_error": error_message,
                "dead_lettered_at": float(dead_lettered_at),
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


async def _wait_for(predicate: Callable[[], bool], *, timeout: float = 2.0) -> None:
    deadline = asyncio.get_running_loop().time() + timeout
    while not predicate():
        if asyncio.get_running_loop().time() >= deadline:
            raise AssertionError("timed out waiting for condition")
        await asyncio.sleep(0.02)


class DashboardEventSubBridgeRuntimeTests(unittest.IsolatedAsyncioTestCase):
    async def test_startup_pending_recovers_without_spending_retry_attempts(self) -> None:
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
                message_id="startup-recovery-1",
                payload={
                    "subscription": {"type": "stream.offline"},
                    "event": {"broadcaster_user_id": "42"},
                },
            )
            await _wait_for(lambda: client.calls and store.rows.get("startup-recovery-1") is not None)
            self.assertEqual(len(client.calls), 1)
            self.assertEqual(store.rows["startup-recovery-1"]["attempt_count"], 0)
            self.assertEqual(
                store.rows["startup-recovery-1"]["last_error"],
                "eventsub notification dispatch inactive",
            )

            current_time["now"] = float(store.rows["startup-recovery-1"]["next_attempt_at"])
            runtime._wakeup.set()
            await _wait_for(lambda: "startup-recovery-1" not in store.rows)

            self.assertEqual(len(client.calls), 2)
            self.assertNotIn("startup-recovery-1", store.dead_letters)
        finally:
            await runtime.stop()

    async def test_non_startup_failure_retries_until_dead_letter(self) -> None:
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
                message_id="dead-letter-retry-1",
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
            await _wait_for(lambda: "dead-letter-retry-1" in store.dead_letters)
        finally:
            await runtime.stop()

        self.assertNotIn("dead-letter-retry-1", store.rows)
        self.assertEqual(store.dead_letters["dead-letter-retry-1"]["attempt_count"], 5)
        self.assertIn("temporary bridge failure", store.dead_letters["dead-letter-retry-1"]["last_error"])
        self.assertEqual(len(client.calls), 5)


class EventSubStateStoreSemanticsTests(unittest.TestCase):
    def test_claim_release_and_expiry_are_isolated_by_kind(self) -> None:
        current_time = {"now": 1000.0}
        store = EventSubStateStore(
            repository=InMemoryEventSubStateRepository(),
            now=lambda: current_time["now"],
        )

        self.assertTrue(store.claim("MESSAGE_ID", "  evt-1  ", ttl_seconds=10.0))
        self.assertTrue(store.is_active("message_id", "evt-1"))
        self.assertFalse(store.claim("message_id", "evt-1", ttl_seconds=10.0))

        self.assertTrue(store.claim("ws_message_id", "evt-1", ttl_seconds=10.0))
        self.assertTrue(store.is_active("ws_message_id", "evt-1"))

        store.release("message_id", "evt-1")
        self.assertFalse(store.is_active("message_id", "evt-1"))
        self.assertTrue(store.is_active("ws_message_id", "evt-1"))

        current_time["now"] = 1011.0
        self.assertFalse(store.is_active("ws_message_id", "evt-1"))
        self.assertTrue(store.claim("ws_message_id", "evt-1", ttl_seconds=10.0))

    def test_empty_or_whitespace_inputs_do_not_create_guards(self) -> None:
        store = EventSubStateStore(
            repository=InMemoryEventSubStateRepository(),
            now=lambda: 1000.0,
        )

        self.assertFalse(store.claim("", "evt-1", ttl_seconds=10.0))
        self.assertFalse(store.claim("message_id", "   ", ttl_seconds=10.0))
        self.assertFalse(store.is_active("", "evt-1"))
        self.assertFalse(store.is_active("message_id", ""))
        store.release("", "evt-1")
        store.release("message_id", "")

    def test_postgres_state_store_retries_once_after_closed_connection_error(self) -> None:
        class _RetryingRepository:
            def __init__(self) -> None:
                self.calls = 0

            def ensure_initialized(self) -> None:
                return None

            def is_active(self, kind: str, key: str, *, now: float) -> bool:
                del kind, key, now
                self.calls += 1
                if self.calls == 1:
                    raise RuntimeError("psycopg.OperationalError: the connection is closed")
                return True

            def claim(self, kind: str, key: str, *, ttl_seconds: float, now: float) -> bool:
                del kind, key, ttl_seconds, now
                return True

            def release(self, kind: str, key: str) -> None:
                del kind, key
                return None

        store = EventSubStateStore(
            logger=logging.getLogger("test.eventsub-state-store"),
            repository=_RetryingRepository(),
            now=lambda: 1000.0,
        )

        with (
            patch.object(store, "_should_retry_after_storage_error", return_value=True),
            patch.object(store, "_reset_postgres_pools") as reset_pools,
        ):
            self.assertTrue(store.is_active("message_id", "evt-1"))

        reset_pools.assert_called_once()
