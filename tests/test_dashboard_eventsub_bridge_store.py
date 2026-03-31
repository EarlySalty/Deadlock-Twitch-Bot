from __future__ import annotations

import contextlib
import json
import unittest
from typing import Any
from unittest.mock import patch

from bot.dashboard_service.eventsub_bridge import DashboardEventSubBridgeStore


class _FakeCursor:
    def __init__(self, *, row: dict[str, Any] | None = None, rows: list[dict[str, Any]] | None = None) -> None:
        self._row = row
        self._rows = list(rows or [])

    def fetchone(self) -> dict[str, Any] | None:
        return self._row

    def fetchall(self) -> list[dict[str, Any]]:
        return list(self._rows)


class _FakeBridgeConnection:
    def __init__(self) -> None:
        self.executed: list[tuple[str, tuple[object, ...]]] = []
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
        sql_text = str(sql or "").strip()
        compact = self._compact_sql(sql_text)
        upper = compact.upper()
        params_tuple = tuple(params or ())
        self.executed.append((sql_text, params_tuple))

        if upper.startswith("CREATE TABLE IF NOT EXISTS") or upper.startswith("CREATE INDEX IF NOT EXISTS"):
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
            due_rows.sort(key=lambda row: (float(row.get("queued_at") or 0.0), str(row.get("message_id") or "")))
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
            message_id, sub_type, payload_json, queued_at, dead_lettered_at, attempt_count, error_message = params_tuple
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


class DashboardEventSubBridgeStoreTests(unittest.TestCase):
    def _patch_transaction(self, conn: _FakeBridgeConnection):
        return patch(
            "bot.dashboard_service.eventsub_bridge.storage.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        )

    def test_ensure_initialized_bootstraps_only_once(self) -> None:
        conn = _FakeBridgeConnection()
        store = DashboardEventSubBridgeStore()

        with self._patch_transaction(conn):
            store.ensure_initialized()
            store.ensure_initialized()

        self.assertEqual(len(conn.executed), 4)
        self.assertTrue(all(sql.startswith(("CREATE TABLE", "CREATE INDEX")) for sql, _ in conn.executed))

    def test_enqueue_inserts_once_and_serializes_payload_stably(self) -> None:
        conn = _FakeBridgeConnection()
        store = DashboardEventSubBridgeStore()

        with self._patch_transaction(conn):
            inserted = store.enqueue(
                message_id="msg-1",
                sub_type="stream.offline",
                payload={"b": 2, "a": 1},
                now=1000.0,
            )
            duplicate = store.enqueue(
                message_id="msg-1",
                sub_type="stream.offline",
                payload={"a": 1, "b": 2},
                now=1001.0,
            )

        self.assertTrue(inserted)
        self.assertFalse(duplicate)
        self.assertEqual(
            conn.outbox["msg-1"],
            {
                "message_id": "msg-1",
                "sub_type": "stream.offline",
                "payload_json": json.dumps({"b": 2, "a": 1}, separators=(",", ":"), sort_keys=True),
                "queued_at": 1000.0,
                "next_attempt_at": 1000.0,
                "attempt_count": 0,
                "last_error": None,
            },
        )
        self.assertEqual(len(conn.executed), 6)
        self.assertEqual(
            sum(1 for sql, _ in conn.executed if sql.startswith("CREATE TABLE") or sql.startswith("CREATE INDEX")),
            4,
        )

    def test_lease_due_returns_due_rows_in_queue_order_and_bumps_next_attempt(self) -> None:
        conn = _FakeBridgeConnection()
        conn.outbox = {
            "msg-2": {
                "message_id": "msg-2",
                "sub_type": "channel.raid",
                "payload_json": '{"id":2}',
                "queued_at": 2.0,
                "next_attempt_at": 100.0,
                "attempt_count": 3,
                "last_error": "previous failure",
            },
            "msg-1": {
                "message_id": "msg-1",
                "sub_type": "stream.offline",
                "payload_json": '{"id":1}',
                "queued_at": 1.0,
                "next_attempt_at": 100.0,
                "attempt_count": 0,
                "last_error": None,
            },
            "msg-future": {
                "message_id": "msg-future",
                "sub_type": "stream.online",
                "payload_json": '{"id":3}',
                "queued_at": 3.0,
                "next_attempt_at": 500.0,
                "attempt_count": 0,
                "last_error": None,
            },
        }
        store = DashboardEventSubBridgeStore()

        with self._patch_transaction(conn):
            leased = store.lease_due(now=100.0, lease_seconds=30.0, limit=2)

        self.assertEqual([row["message_id"] for row in leased], ["msg-1", "msg-2"])
        self.assertEqual(leased[0]["payload_json"], '{"id":1}')
        self.assertEqual(leased[0]["attempt_count"], 0)
        self.assertEqual(conn.outbox["msg-1"]["next_attempt_at"], 130.0)
        self.assertEqual(conn.outbox["msg-2"]["next_attempt_at"], 130.0)
        self.assertEqual(conn.outbox["msg-future"]["next_attempt_at"], 500.0)

    def test_mark_retry_mark_deferred_and_mark_dead_letter_update_backend_state(self) -> None:
        conn = _FakeBridgeConnection()
        conn.outbox = {
            "msg-1": {
                "message_id": "msg-1",
                "sub_type": "channel.raid",
                "payload_json": '{"id":1}',
                "queued_at": 1000.0,
                "next_attempt_at": 1000.0,
                "attempt_count": 0,
                "last_error": None,
            }
        }
        store = DashboardEventSubBridgeStore()

        with self._patch_transaction(conn):
            store.mark_retry(
                message_id="msg-1",
                attempt_count=0,
                error_message="  retry failed\nwith newline  ",
                next_attempt_at=1010.0,
            )
            self.assertEqual(conn.outbox["msg-1"]["attempt_count"], 1)
            self.assertEqual(conn.outbox["msg-1"]["next_attempt_at"], 1010.0)
            self.assertEqual(conn.outbox["msg-1"]["last_error"], "retry failed with newline")

            store.mark_deferred(
                message_id="msg-1",
                error_message="startup pending",
                next_attempt_at=1020.0,
            )
            self.assertEqual(conn.outbox["msg-1"]["attempt_count"], 1)
            self.assertEqual(conn.outbox["msg-1"]["next_attempt_at"], 1020.0)
            self.assertEqual(conn.outbox["msg-1"]["last_error"], "startup pending")

            store.mark_dead_letter(
                message_id="msg-1",
                sub_type="channel.raid",
                payload_json='{"id":1}',
                queued_at=1000.0,
                attempt_count=7,
                error_message="permanent failure",
                dead_lettered_at=1030.0,
            )

        self.assertNotIn("msg-1", conn.outbox)
        self.assertEqual(
            conn.dead_letters["msg-1"],
            {
                "message_id": "msg-1",
                "sub_type": "channel.raid",
                "payload_json": '{"id":1}',
                "queued_at": 1000.0,
                "dead_lettered_at": 1030.0,
                "attempt_count": 7,
                "last_error": "permanent failure",
            },
        )
        self.assertTrue(
            any("SET attempt_count" in sql for sql, _ in conn.executed if sql.startswith("UPDATE twitch_eventsub_bridge_outbox"))
        )
        self.assertTrue(
            any("SET next_attempt_at" in sql for sql, _ in conn.executed if sql.startswith("UPDATE twitch_eventsub_bridge_outbox"))
        )
        self.assertTrue(
            any(sql.startswith("INSERT INTO twitch_eventsub_bridge_dead_letter") for sql, _ in conn.executed)
        )

    def test_mark_dead_letter_overwrites_existing_dead_letter_row(self) -> None:
        conn = _FakeBridgeConnection()
        store = DashboardEventSubBridgeStore()

        with self._patch_transaction(conn):
            store.mark_dead_letter(
                message_id="msg-1",
                sub_type="stream.offline",
                payload_json='{"id":1}',
                queued_at=1000.0,
                attempt_count=1,
                error_message="first failure",
                dead_lettered_at=1010.0,
            )
            store.mark_dead_letter(
                message_id="msg-1",
                sub_type="stream.offline",
                payload_json='{"id":2}',
                queued_at=1001.0,
                attempt_count=2,
                error_message="second failure",
                dead_lettered_at=1020.0,
            )

        self.assertEqual(conn.outbox, {})
        self.assertEqual(
            conn.dead_letters["msg-1"],
            {
                "message_id": "msg-1",
                "sub_type": "stream.offline",
                "payload_json": '{"id":2}',
                "queued_at": 1001.0,
                "dead_lettered_at": 1020.0,
                "attempt_count": 2,
                "last_error": "second failure",
            },
        )
