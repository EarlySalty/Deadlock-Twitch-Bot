from __future__ import annotations

import contextlib
import json
import unittest
from typing import Any
from unittest.mock import patch

from bot.dashboard_service.eventsub_bridge import DashboardEventSubBridgeStore
from bot.storage import pg as storage_pg


class _FakeCursor:
    def __init__(
        self,
        *,
        row: dict[str, Any] | None = None,
        rows: list[dict[str, Any]] | None = None,
    ) -> None:
        self._row = row
        self._rows = list(rows or [])
        self.rowcount = 0

    def fetchone(self) -> dict[str, Any] | None:
        return self._row

    def fetchall(self) -> list[dict[str, Any]]:
        return list(self._rows)


class _FakeBridgePgConnection:
    def __init__(self, *, fail_on_sql: tuple[str, ...] = ()) -> None:
        self.fail_on_sql = tuple(token.upper() for token in fail_on_sql)
        self.executed: list[tuple[str, tuple[object, ...]]] = []
        self.commit_calls = 0
        self.rollback_calls = 0
        self.outbox: dict[str, dict[str, Any]] = {}
        self.dead_letters: dict[str, dict[str, Any]] = {}

    @staticmethod
    def _compact_sql(sql: str) -> str:
        return " ".join(str(sql or "").strip().split())

    @staticmethod
    def _trim_error(value: object) -> str | None:
        text = str(value or "").strip().replace("\r", " ").replace("\n", " ")
        if not text:
            return None
        return text[:500]

    def execute(self, sql: str, params=(), *args, **kwargs) -> _FakeCursor:
        del args, kwargs
        sql_text = str(sql or "")
        compact = self._compact_sql(sql_text)
        upper = compact.upper()
        params_tuple = tuple(params or ())
        self.executed.append((compact, params_tuple))

        if any(token in upper for token in self.fail_on_sql):
            raise RuntimeError("sql boom")

        if upper.startswith("CREATE TABLE IF NOT EXISTS") or upper.startswith(
            "CREATE INDEX IF NOT EXISTS"
        ):
            return _FakeCursor()

        if "INSERT INTO TWITCH_EVENTSUB_BRIDGE_OUTBOX" in upper and "RETURNING MESSAGE_ID" in upper:
            message_id, sub_type, payload_json, queued_at, next_attempt_at = params_tuple
            message_id = str(message_id)
            if message_id in self.outbox:
                return _FakeCursor(row=None)
            self.outbox[message_id] = {
                "message_id": message_id,
                "sub_type": str(sub_type),
                "payload_json": str(payload_json),
                "queued_at": float(queued_at),
                "next_attempt_at": float(next_attempt_at),
                "attempt_count": 0,
                "last_error": None,
            }
            return _FakeCursor(row={"message_id": message_id})

        if "FOR UPDATE SKIP LOCKED" in upper and "RETURNING OUTBOX.MESSAGE_ID" in upper:
            now, limit, lease_until = params_tuple
            due_rows = [
                row
                for row in self.outbox.values()
                if float(row.get("next_attempt_at") or 0.0) <= float(now)
            ]
            due_rows.sort(
                key=lambda row: (
                    float(row.get("queued_at") or 0.0),
                    str(row.get("message_id") or ""),
                )
            )
            selected = due_rows[: max(1, int(limit))]
            rows: list[dict[str, Any]] = []
            for row in selected:
                row["next_attempt_at"] = float(lease_until)
                rows.append(
                    {
                        "message_id": row["message_id"],
                        "sub_type": row["sub_type"],
                        "payload_json": row["payload_json"],
                        "queued_at": row["queued_at"],
                        "attempt_count": row["attempt_count"],
                    }
                )
            return _FakeCursor(rows=rows)

        if upper.startswith("DELETE FROM TWITCH_EVENTSUB_BRIDGE_OUTBOX WHERE MESSAGE_ID = %S"):
            message_id = str(params_tuple[0])
            self.outbox.pop(message_id, None)
            return _FakeCursor()

        if upper.startswith("UPDATE TWITCH_EVENTSUB_BRIDGE_OUTBOX") and "SET ATTEMPT_COUNT" in upper:
            attempt_count, next_attempt_at, error_message, message_id = params_tuple
            row = self.outbox.get(str(message_id))
            if row is not None:
                row["attempt_count"] = max(1, int(attempt_count))
                row["next_attempt_at"] = float(next_attempt_at)
                row["last_error"] = self._trim_error(error_message)
            return _FakeCursor()

        if upper.startswith("UPDATE TWITCH_EVENTSUB_BRIDGE_OUTBOX") and "SET NEXT_ATTEMPT_AT" in upper:
            next_attempt_at, error_message, message_id = params_tuple
            row = self.outbox.get(str(message_id))
            if row is not None:
                row["next_attempt_at"] = float(next_attempt_at)
                row["last_error"] = self._trim_error(error_message)
            return _FakeCursor()

        if upper.startswith("INSERT INTO TWITCH_EVENTSUB_BRIDGE_DEAD_LETTER"):
            (
                message_id,
                sub_type,
                payload_json,
                queued_at,
                dead_lettered_at,
                attempt_count,
                error_message,
            ) = params_tuple
            message_id = str(message_id)
            self.dead_letters[message_id] = {
                "message_id": message_id,
                "sub_type": str(sub_type),
                "payload_json": str(payload_json),
                "queued_at": float(queued_at),
                "dead_lettered_at": float(dead_lettered_at),
                "attempt_count": max(1, int(attempt_count)),
                "last_error": self._trim_error(error_message),
            }
            self.outbox.pop(message_id, None)
            return _FakeCursor()

        raise AssertionError(f"unexpected SQL: {sql_text}")

    def commit(self) -> None:
        self.commit_calls += 1

    def rollback(self) -> None:
        self.rollback_calls += 1


class _FakePoolConnectionContext:
    def __init__(self, pool: "_FakeBridgePgPool", *, autocommit: bool) -> None:
        self._pool = pool
        self._autocommit = autocommit

    def __enter__(self) -> _FakeBridgePgConnection:
        self._pool.requested_autocommit.append(self._autocommit)
        return self._pool._connection

    def __exit__(self, exc_type, exc, tb) -> bool:
        del exc_type, exc, tb
        return False


class _FakeBridgePgPool:
    def __init__(self, connection: _FakeBridgePgConnection) -> None:
        self._connection = connection
        self.requested_autocommit: list[bool] = []

    def connection(self, *, autocommit: bool) -> _FakePoolConnectionContext:
        return _FakePoolConnectionContext(self, autocommit=autocommit)


class _FakeBridgePgRegistry:
    def __init__(self, pool: _FakeBridgePgPool) -> None:
        self.pool = pool
        self.requested_dsns: list[str] = []

    def get_pool(self, dsn: str) -> _FakeBridgePgPool:
        self.requested_dsns.append(dsn)
        return self.pool


class DashboardEventSubBridgeStorePgRuntimeTests(unittest.TestCase):
    def setUp(self) -> None:
        self.dsn = "postgresql://demo@host:5432/db"
        self.cache_key = storage_pg.analytics_db_fingerprint(self.dsn)

    def tearDown(self) -> None:
        with contextlib.suppress(Exception):
            storage_pg._reset_connection_pools()

    def _mark_runtime_ready(self) -> None:
        storage_pg._mark_runtime_storage_ready(self.cache_key)

    def _patch_runtime_storage(
        self,
        connection: _FakeBridgePgConnection,
    ) -> tuple[_FakeBridgePgRegistry, _FakeBridgePgPool]:
        pool = _FakeBridgePgPool(connection)
        registry = _FakeBridgePgRegistry(pool)
        return registry, pool

    def test_store_rejects_operations_before_runtime_storage_is_ready(self) -> None:
        connection = _FakeBridgePgConnection()
        registry, pool = self._patch_runtime_storage(connection)
        store = DashboardEventSubBridgeStore()

        with (
            patch("bot.storage.pg._load_dsn", return_value=self.dsn),
            patch("bot.storage.pg._connection_pool_registry", return_value=registry),
            self.assertRaisesRegex(
                RuntimeError,
                "PostgreSQL storage is not initialized",
            ),
        ):
            store.enqueue(
                message_id="msg-unready",
                sub_type="stream.offline",
                payload={"event": "pending"},
                now=1000.0,
            )

        self.assertEqual(registry.requested_dsns, [])
        self.assertEqual(pool.requested_autocommit, [])
        self.assertEqual(connection.commit_calls, 0)
        self.assertEqual(connection.rollback_calls, 0)

    def test_store_methods_use_the_real_transaction_context_and_commit_on_success(self) -> None:
        connection = _FakeBridgePgConnection()
        registry, pool = self._patch_runtime_storage(connection)
        store = DashboardEventSubBridgeStore()

        connection.outbox.update(
            {
                "lease-1": {
                    "message_id": "lease-1",
                    "sub_type": "stream.offline",
                    "payload_json": '{"id":1}',
                    "queued_at": 1.0,
                    "next_attempt_at": 100.0,
                    "attempt_count": 0,
                    "last_error": None,
                },
                "lease-2": {
                    "message_id": "lease-2",
                    "sub_type": "channel.raid",
                    "payload_json": '{"id":2}',
                    "queued_at": 2.0,
                    "next_attempt_at": 100.0,
                    "attempt_count": 3,
                    "last_error": "previous failure",
                },
                "lease-future": {
                    "message_id": "lease-future",
                    "sub_type": "stream.online",
                    "payload_json": '{"id":3}',
                    "queued_at": 3.0,
                    "next_attempt_at": 500.0,
                    "attempt_count": 0,
                    "last_error": None,
                },
                "retry-1": {
                    "message_id": "retry-1",
                    "sub_type": "channel.raid",
                    "payload_json": '{"id":4}',
                    "queued_at": 4.0,
                    "next_attempt_at": 100.0,
                    "attempt_count": 0,
                    "last_error": None,
                },
                "defer-1": {
                    "message_id": "defer-1",
                    "sub_type": "stream.online",
                    "payload_json": '{"id":5}',
                    "queued_at": 5.0,
                    "next_attempt_at": 100.0,
                    "attempt_count": 0,
                    "last_error": None,
                },
                "dead-1": {
                    "message_id": "dead-1",
                    "sub_type": "channel.raid",
                    "payload_json": '{"id":6}',
                    "queued_at": 6.0,
                    "next_attempt_at": 100.0,
                    "attempt_count": 4,
                    "last_error": "previous failure",
                },
                "deliver-1": {
                    "message_id": "deliver-1",
                    "sub_type": "stream.offline",
                    "payload_json": '{"id":7}',
                    "queued_at": 7.0,
                    "next_attempt_at": 100.0,
                    "attempt_count": 0,
                    "last_error": None,
                },
            }
        )

        with (
            patch("bot.storage.pg._load_dsn", return_value=self.dsn),
            patch("bot.storage.pg._connection_pool_registry", return_value=registry),
        ):
            self._mark_runtime_ready()

            store.ensure_initialized()
            inserted = store.enqueue(
                message_id="enqueue-1",
                sub_type="stream.offline",
                payload={"b": 2, "a": 1},
                now=1000.0,
            )
            duplicate = store.enqueue(
                message_id="enqueue-1",
                sub_type="stream.offline",
                payload={"a": 1, "b": 2},
                now=1001.0,
            )
            leased = store.lease_due(now=100.0, lease_seconds=30.0, limit=2)
            store.mark_retry(
                message_id="retry-1",
                attempt_count=0,
                error_message="  retry failed\nwith newline  ",
                next_attempt_at=110.0,
            )
            store.mark_deferred(
                message_id="defer-1",
                error_message="startup pending",
                next_attempt_at=120.0,
            )
            store.mark_dead_letter(
                message_id="dead-1",
                sub_type="channel.raid",
                payload_json='{"id":6}',
                queued_at=6.0,
                attempt_count=7,
                error_message="permanent failure",
                dead_lettered_at=130.0,
            )
            store.mark_delivered(message_id="deliver-1")

        self.assertTrue(inserted)
        self.assertFalse(duplicate)
        self.assertEqual(
            connection.outbox["enqueue-1"],
            {
                "message_id": "enqueue-1",
                "sub_type": "stream.offline",
                "payload_json": json.dumps(
                    {"b": 2, "a": 1},
                    separators=(",", ":"),
                    sort_keys=True,
                ),
                "queued_at": 1000.0,
                "next_attempt_at": 1000.0,
                "attempt_count": 0,
                "last_error": None,
            },
        )
        self.assertEqual([row["message_id"] for row in leased], ["lease-1", "lease-2"])
        self.assertEqual(connection.outbox["lease-1"]["next_attempt_at"], 130.0)
        self.assertEqual(connection.outbox["lease-2"]["next_attempt_at"], 130.0)
        self.assertEqual(connection.outbox["lease-future"]["next_attempt_at"], 500.0)
        self.assertEqual(connection.outbox["retry-1"]["attempt_count"], 1)
        self.assertEqual(connection.outbox["retry-1"]["last_error"], "retry failed with newline")
        self.assertEqual(connection.outbox["retry-1"]["next_attempt_at"], 110.0)
        self.assertEqual(connection.outbox["defer-1"]["last_error"], "startup pending")
        self.assertEqual(connection.outbox["defer-1"]["next_attempt_at"], 120.0)
        self.assertNotIn("dead-1", connection.outbox)
        self.assertEqual(
            connection.dead_letters["dead-1"],
            {
                "message_id": "dead-1",
                "sub_type": "channel.raid",
                "payload_json": '{"id":6}',
                "queued_at": 6.0,
                "dead_lettered_at": 130.0,
                "attempt_count": 7,
                "last_error": "permanent failure",
            },
        )
        self.assertNotIn("deliver-1", connection.outbox)
        self.assertEqual(connection.commit_calls, 8)
        self.assertEqual(connection.rollback_calls, 0)
        self.assertEqual(pool.requested_autocommit, [False] * 8)
        self.assertEqual(registry.requested_dsns, [self.dsn] * 8)

    def test_sql_error_rolls_back_the_transaction_instead_of_committing(self) -> None:
        connection = _FakeBridgePgConnection(
            fail_on_sql=("INSERT INTO TWITCH_EVENTSUB_BRIDGE_OUTBOX",)
        )
        registry, pool = self._patch_runtime_storage(connection)
        store = DashboardEventSubBridgeStore()

        with (
            patch("bot.storage.pg._load_dsn", return_value=self.dsn),
            patch("bot.storage.pg._connection_pool_registry", return_value=registry),
        ):
            self._mark_runtime_ready()
            store.ensure_initialized()

            with self.assertRaisesRegex(RuntimeError, "sql boom"):
                store.enqueue(
                    message_id="enqueue-rollback",
                    sub_type="stream.offline",
                    payload={"x": 1},
                    now=2000.0,
                )

        self.assertEqual(connection.commit_calls, 1)
        self.assertEqual(connection.rollback_calls, 1)
        self.assertNotIn("enqueue-rollback", connection.outbox)
        self.assertEqual(pool.requested_autocommit, [False, False])
        self.assertEqual(registry.requested_dsns, [self.dsn, self.dsn])
