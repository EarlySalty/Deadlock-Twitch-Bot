"""Tests für das Voice-Reaction-Audit-Log."""

import contextlib
import json
import sqlite3
import unittest
from datetime import UTC, datetime

from bot.community.voice_reaction.audit_log import audit, new_correlation_id


class _CompatCursor:
    def __init__(self, *, rowcount: int = 0, rows=None) -> None:
        self.rowcount = rowcount
        self._rows = list(rows or [])

    def fetchone(self):
        return self._rows[0] if self._rows else None

    def fetchall(self):
        return list(self._rows)


class _CompatConn:
    """Übersetzt PG-spezifisches SQL minimal nach SQLite."""

    def __init__(self, conn: sqlite3.Connection, *, fail: bool = False) -> None:
        self._conn = conn
        self._fail = fail

    def execute(self, sql: str, params=None):
        if self._fail:
            raise RuntimeError("db down")

        sql_text = str(sql or "").strip()
        values = tuple(params or ())
        if "RETURNING id" in sql_text:
            return self._execute_insert_returning_id(sql_text, values)

        sql_text = sql_text.replace("%s", "?")
        sql_text = sql_text.replace("NOW()", "datetime('now')")
        sql_text = sql_text.replace("::jsonb", "")
        sql_text = sql_text.replace("::uuid", "")
        return self._conn.execute(sql_text, values)

    def commit(self):
        return self._conn.commit()

    def _execute_insert_returning_id(self, sql: str, params: tuple[object, ...]):
        sql_text = sql.replace("RETURNING id", "")
        sql_text = sql_text.replace("%s", "?")
        sql_text = sql_text.replace("::jsonb", "")
        sql_text = sql_text.replace("::uuid", "")
        cursor = self._conn.execute(sql_text, params)
        return _CompatCursor(rowcount=cursor.rowcount, rows=[{"id": cursor.lastrowid}])

    def __getattr__(self, item):
        return getattr(self._conn, item)


def _make_conn() -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:")
    conn.row_factory = sqlite3.Row
    conn.execute(
        """
        CREATE TABLE twitch_partner_outreach (
            streamer_login TEXT PRIMARY KEY,
            streamer_user_id TEXT,
            detected_at TEXT,
            contacted_at TEXT,
            status TEXT,
            cooldown_until TEXT,
            notes TEXT,
            raid_used_at TEXT,
            conversation_status TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE twitch_partner_outreach_audit (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            streamer_login  TEXT NOT NULL,
            occurred_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            event_kind      TEXT NOT NULL,
            payload_json    TEXT NOT NULL DEFAULT '{}',
            correlation_id  TEXT,
            FOREIGN KEY (streamer_login) REFERENCES twitch_partner_outreach(streamer_login) ON DELETE CASCADE
        )
        """
    )
    conn.execute(
        "INSERT INTO twitch_partner_outreach (streamer_login, streamer_user_id, status) VALUES (?, ?, ?)",
        ("auditcase", "100", "sent"),
    )
    conn.commit()
    return conn


class VoiceReactionAuditLogTests(unittest.TestCase):
    def test_audit_writes_row_with_payload_and_correlation(self) -> None:
        conn = _make_conn()
        correlation_id = new_correlation_id()

        row_id = audit(
            "auditcase",
            "brain_call_output",
            {"answer": "yes"},
            correlation_id=correlation_id,
            transaction_factory=lambda: contextlib.nullcontext(_CompatConn(conn)),
        )

        self.assertIsInstance(row_id, int)
        row = conn.execute(
            """
            SELECT streamer_login, event_kind, payload_json, correlation_id
            FROM twitch_partner_outreach_audit
            WHERE id = ?
            """,
            (row_id,),
        ).fetchone()
        self.assertEqual(row["streamer_login"], "auditcase")
        self.assertEqual(row["event_kind"], "brain_call_output")
        self.assertEqual(json.loads(row["payload_json"]), {"answer": "yes"})
        self.assertEqual(row["correlation_id"], correlation_id)

    def test_audit_serializes_datetime_in_payload(self) -> None:
        conn = _make_conn()
        captured_at = datetime.now(UTC)

        row_id = audit(
            "auditcase",
            "voice_capture_done",
            {"captured_at": captured_at},
            transaction_factory=lambda: contextlib.nullcontext(_CompatConn(conn)),
        )

        row = conn.execute(
            "SELECT payload_json FROM twitch_partner_outreach_audit WHERE id = ?",
            (row_id,),
        ).fetchone()
        payload = json.loads(row["payload_json"])
        self.assertEqual(payload["captured_at"], str(captured_at))

    def test_audit_without_correlation_id_writes_null(self) -> None:
        conn = _make_conn()

        row_id = audit(
            "auditcase",
            "streamer_chat_received",
            {"text": "hey"},
            transaction_factory=lambda: contextlib.nullcontext(_CompatConn(conn)),
        )

        row = conn.execute(
            "SELECT correlation_id FROM twitch_partner_outreach_audit WHERE id = ?",
            (row_id,),
        ).fetchone()
        self.assertIsNone(row["correlation_id"])

    def test_audit_returns_id_on_success(self) -> None:
        conn = _make_conn()

        row_id = audit(
            "auditcase",
            "conversation_opened",
            {},
            transaction_factory=lambda: contextlib.nullcontext(_CompatConn(conn)),
        )

        self.assertIsInstance(row_id, int)
        self.assertGreater(row_id, 0)

    def test_audit_returns_none_on_db_error_and_does_not_raise(self) -> None:
        conn = _make_conn()

        result = audit(
            "auditcase",
            "brain_failed",
            {"error": "boom"},
            transaction_factory=lambda: contextlib.nullcontext(_CompatConn(conn, fail=True)),
        )

        self.assertIsNone(result)

    def test_new_correlation_id_returns_unique_hex_strings(self) -> None:
        first = new_correlation_id()
        second = new_correlation_id()

        self.assertEqual(len(first), 32)
        self.assertEqual(len(second), 32)
        self.assertNotEqual(first, second)
        int(first, 16)
        int(second, 16)


if __name__ == "__main__":
    unittest.main()
