"""Tests für den Voice-Reaction-State-Store."""

import contextlib
import json
import sqlite3
import unittest
from datetime import datetime, timedelta

from bot.community.voice_reaction.state_store import (
    append_message,
    close_conversation,
    get_conversation,
    has_active_conversation,
    load_active_conversations,
    open_conversation,
    update_state,
)


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

        if "messages_json = messages_json || %s::jsonb" in sql_text:
            return self._append_messages(sql_text, values)

        if "SET cooldown_until = GREATEST(" in sql_text:
            return self._extend_cooldown(values)

        sql_text = sql_text.replace(
            "INSERT INTO twitch_partner_outreach_conversations",
            "INSERT OR IGNORE INTO twitch_partner_outreach_conversations",
        )
        sql_text = sql_text.replace("ON CONFLICT (streamer_login) DO NOTHING", "")
        sql_text = sql_text.replace("%s", "?")
        sql_text = sql_text.replace("NOW()", "datetime('now')")
        sql_text = sql_text.replace("::jsonb", "")
        sql_text = sql_text.replace("::uuid", "")
        sql_text = sql_text.replace("::text", "")
        sql_text = sql_text.replace("::timestamptz", "")
        return self._conn.execute(sql_text, values)

    def commit(self):
        return self._conn.commit()

    def _append_messages(self, sql: str, params: tuple[object, ...]):
        payload_json = str(params[0])
        streamer_login = str(params[1])
        row = self._conn.execute(
            "SELECT messages_json FROM twitch_partner_outreach_conversations WHERE streamer_login = ?",
            (streamer_login,),
        ).fetchone()
        if row is None:
            return _CompatCursor(rowcount=0)

        existing = json.loads(row["messages_json"] or "[]")
        addition = json.loads(payload_json or "[]")
        merged = list(existing) + list(addition)
        assignments = ["messages_json = ?", "updated_at = CURRENT_TIMESTAMP"]
        if "last_voice_capture_at = NOW()" in sql:
            assignments.append("last_voice_capture_at = CURRENT_TIMESTAMP")
        if "last_streamer_signal_at = NOW()" in sql:
            assignments.append("last_streamer_signal_at = CURRENT_TIMESTAMP")
        if "last_bot_message_at = NOW()" in sql:
            assignments.append("last_bot_message_at = CURRENT_TIMESTAMP")
        cursor = self._conn.execute(
            f"""
            UPDATE twitch_partner_outreach_conversations
               SET {", ".join(assignments)}
             WHERE streamer_login = ?
            """,
            (json.dumps(merged, separators=(",", ":")), streamer_login),
        )
        return _CompatCursor(rowcount=cursor.rowcount)

    def _extend_cooldown(self, params: tuple[object, ...]):
        days = int(str(params[0]))
        streamer_login = str(params[1])
        row = self._conn.execute(
            "SELECT cooldown_until FROM twitch_partner_outreach WHERE streamer_login = ?",
            (streamer_login,),
        ).fetchone()
        if row is None:
            return _CompatCursor(rowcount=0)

        now_dt = datetime.now()
        target_dt = now_dt + timedelta(days=days)
        current_raw = row["cooldown_until"]
        current_dt = datetime.fromisoformat(current_raw) if current_raw else now_dt
        winner = max(current_dt, target_dt)
        cursor = self._conn.execute(
            """
            UPDATE twitch_partner_outreach
               SET cooldown_until = ?, conversation_status = 'closed'
             WHERE streamer_login = ?
            """,
            (winner.isoformat(sep=" "), streamer_login),
        )
        return _CompatCursor(rowcount=cursor.rowcount)

    def __getattr__(self, item):
        return getattr(self._conn, item)


def _make_conn() -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:")
    conn.row_factory = sqlite3.Row
    conn.execute(
        """
        CREATE TABLE twitch_partner_outreach (
            streamer_login      TEXT PRIMARY KEY,
            streamer_user_id    TEXT,
            detected_at         TEXT,
            contacted_at        TEXT,
            status              TEXT,
            cooldown_until      TEXT,
            notes               TEXT,
            raid_used_at        TEXT,
            conversation_status TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE twitch_partner_outreach_conversations (
            streamer_login          TEXT PRIMARY KEY,
            streamer_user_id        TEXT,
            source                  TEXT NOT NULL,
            state                   TEXT NOT NULL DEFAULT 'open',
            messages_json           TEXT NOT NULL DEFAULT '[]',
            last_voice_capture_at   TEXT,
            last_brain_call_at      TEXT,
            last_bot_message_at     TEXT,
            last_streamer_signal_at TEXT,
            last_stance             TEXT,
            last_confidence         REAL,
            human_notify_sent_at    TEXT,
            human_notify_pending_at TEXT,
            closed_at               TEXT,
            error_kind              TEXT,
            error_detail            TEXT,
            created_at              TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at              TEXT DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    return conn


class VoiceReactionStateStoreTests(unittest.TestCase):
    def test_open_conversation_inserts_row_and_marks_outreach(self) -> None:
        conn = _make_conn()
        conn.execute(
            "INSERT INTO twitch_partner_outreach (streamer_login, streamer_user_id, status) VALUES (?, ?, ?)",
            ("alpha", "100", "sent"),
        )
        conn.commit()

        created = open_conversation(
            streamer_login="alpha",
            streamer_user_id="100",
            source="outreach",
            transaction_factory=lambda: contextlib.nullcontext(_CompatConn(conn)),
        )

        self.assertTrue(created)
        row = conn.execute(
            "SELECT state, messages_json FROM twitch_partner_outreach_conversations WHERE streamer_login = ?",
            ("alpha",),
        ).fetchone()
        self.assertEqual(row["state"], "open")
        self.assertEqual(row["messages_json"], "[]")
        outreach = conn.execute(
            "SELECT conversation_status FROM twitch_partner_outreach WHERE streamer_login = ?",
            ("alpha",),
        ).fetchone()
        self.assertEqual(outreach["conversation_status"], "open")

    def test_open_conversation_idempotent_on_existing_row(self) -> None:
        conn = _make_conn()
        conn.execute(
            "INSERT INTO twitch_partner_outreach (streamer_login, streamer_user_id, status) VALUES (?, ?, ?)",
            ("beta", "200", "sent"),
        )
        conn.commit()
        def factory():
            return contextlib.nullcontext(_CompatConn(conn))

        first = open_conversation(
            streamer_login="beta",
            streamer_user_id="200",
            source="outreach",
            transaction_factory=factory,
        )
        second = open_conversation(
            streamer_login="beta",
            streamer_user_id="999",
            source="raid_boost",
            transaction_factory=factory,
        )

        self.assertTrue(first)
        self.assertFalse(second)
        row = conn.execute(
            "SELECT streamer_user_id, source FROM twitch_partner_outreach_conversations WHERE streamer_login = ?",
            ("beta",),
        ).fetchone()
        self.assertEqual(row["streamer_user_id"], "200")
        self.assertEqual(row["source"], "outreach")

    def test_append_message_voice_updates_last_voice_capture_at(self) -> None:
        conn = _make_conn()
        self._seed_conversation(conn, "voicecase")

        updated = append_message(
            streamer_login="voicecase",
            role="voice",
            text="hey there",
            transaction_factory=lambda: contextlib.nullcontext(_CompatConn(conn)),
        )

        self.assertTrue(updated)
        row = conn.execute(
            "SELECT messages_json, last_voice_capture_at FROM twitch_partner_outreach_conversations WHERE streamer_login = ?",
            ("voicecase",),
        ).fetchone()
        messages = json.loads(row["messages_json"])
        self.assertEqual(len(messages), 1)
        self.assertEqual(messages[0]["role"], "voice")
        self.assertIsNotNone(row["last_voice_capture_at"])

    def test_append_message_streamer_chat_updates_last_streamer_signal_at(self) -> None:
        conn = _make_conn()
        self._seed_conversation(conn, "chatcase")

        updated = append_message(
            streamer_login="chatcase",
            role="streamer_chat",
            text="hello bot",
            transaction_factory=lambda: contextlib.nullcontext(_CompatConn(conn)),
        )

        self.assertTrue(updated)
        row = conn.execute(
            "SELECT last_streamer_signal_at FROM twitch_partner_outreach_conversations WHERE streamer_login = ?",
            ("chatcase",),
        ).fetchone()
        self.assertIsNotNone(row["last_streamer_signal_at"])

    def test_append_message_bot_chat_updates_last_bot_message_at(self) -> None:
        conn = _make_conn()
        self._seed_conversation(conn, "botcase")

        updated = append_message(
            streamer_login="botcase",
            role="bot_chat",
            text="thanks",
            transaction_factory=lambda: contextlib.nullcontext(_CompatConn(conn)),
        )

        self.assertTrue(updated)
        row = conn.execute(
            "SELECT last_bot_message_at FROM twitch_partner_outreach_conversations WHERE streamer_login = ?",
            ("botcase",),
        ).fetchone()
        self.assertIsNotNone(row["last_bot_message_at"])

    def test_append_message_returns_false_when_no_conversation(self) -> None:
        conn = _make_conn()

        updated = append_message(
            streamer_login="missing",
            role="voice",
            text="ghost",
            transaction_factory=lambda: contextlib.nullcontext(_CompatConn(conn)),
        )

        self.assertFalse(updated)

    def test_update_state_changes_state_and_optional_fields(self) -> None:
        conn = _make_conn()
        self._seed_conversation(conn, "statecase")

        updated = update_state(
            streamer_login="statecase",
            new_state="brain_pending",
            last_stance="questioning",
            last_confidence=0.82,
            error_kind="soft",
            error_detail="retry later",
            transaction_factory=lambda: contextlib.nullcontext(_CompatConn(conn)),
        )

        self.assertTrue(updated)
        row = conn.execute(
            """
            SELECT state, last_stance, last_confidence, error_kind, error_detail
            FROM twitch_partner_outreach_conversations
            WHERE streamer_login = ?
            """,
            ("statecase",),
        ).fetchone()
        self.assertEqual(row["state"], "brain_pending")
        self.assertEqual(row["last_stance"], "questioning")
        self.assertEqual(row["last_confidence"], 0.82)
        self.assertEqual(row["error_kind"], "soft")
        self.assertEqual(row["error_detail"], "retry later")

    def test_close_conversation_sets_closed_state_and_marks_outreach(self) -> None:
        conn = _make_conn()
        self._seed_conversation(conn, "closecase")

        closed = close_conversation(
            streamer_login="closecase",
            close_reason="declined",
            transaction_factory=lambda: contextlib.nullcontext(_CompatConn(conn)),
        )

        self.assertTrue(closed)
        convo = conn.execute(
            "SELECT state, closed_at FROM twitch_partner_outreach_conversations WHERE streamer_login = ?",
            ("closecase",),
        ).fetchone()
        outreach = conn.execute(
            "SELECT conversation_status FROM twitch_partner_outreach WHERE streamer_login = ?",
            ("closecase",),
        ).fetchone()
        self.assertEqual(convo["state"], "closed_declined")
        self.assertIsNotNone(convo["closed_at"])
        self.assertEqual(outreach["conversation_status"], "closed")

    def test_close_conversation_with_cooldown_extends_cooldown_until(self) -> None:
        conn = _make_conn()
        self._seed_conversation(conn, "cooldowncase")

        closed = close_conversation(
            streamer_login="cooldowncase",
            close_reason="no_signal",
            extend_cooldown_days=3,
            transaction_factory=lambda: contextlib.nullcontext(_CompatConn(conn)),
        )

        self.assertTrue(closed)
        row = conn.execute(
            "SELECT cooldown_until FROM twitch_partner_outreach WHERE streamer_login = ?",
            ("cooldowncase",),
        ).fetchone()
        cooldown = datetime.fromisoformat(row["cooldown_until"])
        self.assertGreaterEqual(cooldown, datetime.now() + timedelta(days=3, seconds=-5))

    def test_close_conversation_with_cooldown_does_not_shorten_existing_cooldown(self) -> None:
        conn = _make_conn()
        self._seed_conversation(conn, "futurecase")
        future = (datetime.now() + timedelta(days=10)).isoformat(sep=" ")
        conn.execute(
            "UPDATE twitch_partner_outreach SET cooldown_until = ? WHERE streamer_login = ?",
            (future, "futurecase"),
        )
        conn.commit()

        closed = close_conversation(
            streamer_login="futurecase",
            close_reason="error",
            extend_cooldown_days=2,
            transaction_factory=lambda: contextlib.nullcontext(_CompatConn(conn)),
        )

        self.assertTrue(closed)
        row = conn.execute(
            "SELECT cooldown_until FROM twitch_partner_outreach WHERE streamer_login = ?",
            ("futurecase",),
        ).fetchone()
        self.assertEqual(row["cooldown_until"], future)

    def test_get_conversation_returns_parsed_messages(self) -> None:
        conn = _make_conn()
        self._seed_conversation(
            conn,
            "lookupcase",
            messages=[{"role": "system", "ts": "2026-01-01T00:00:00+00:00", "text": "hi", "meta": {}}],
        )

        result = get_conversation(
            streamer_login="lookupcase",
            connection_factory=lambda: contextlib.nullcontext(_CompatConn(conn)),
        )

        self.assertIsNotNone(result)
        self.assertEqual(result["streamer_login"], "lookupcase")
        self.assertEqual(result["messages_json"][0]["text"], "hi")

    def test_get_conversation_returns_none_when_missing(self) -> None:
        conn = _make_conn()

        result = get_conversation(
            streamer_login="missing",
            connection_factory=lambda: contextlib.nullcontext(_CompatConn(conn)),
        )

        self.assertIsNone(result)

    def test_load_active_conversations_filters_open_and_listening(self) -> None:
        conn = _make_conn()
        self._seed_conversation(conn, "openone", state="open")
        self._seed_conversation(conn, "listenone", state="listening")
        self._seed_conversation(conn, "closedone", state="closed_declined")

        rows = load_active_conversations(
            connection_factory=lambda: contextlib.nullcontext(_CompatConn(conn)),
        )

        self.assertEqual({row["streamer_login"] for row in rows}, {"openone", "listenone"})

    def test_has_active_conversation_true_for_open_false_for_closed(self) -> None:
        conn = _make_conn()
        self._seed_conversation(conn, "activeone", state="open")
        self._seed_conversation(conn, "inactiveone", state="closed_declined")
        def factory():
            return contextlib.nullcontext(_CompatConn(conn))

        self.assertTrue(has_active_conversation("activeone", connection_factory=factory))
        self.assertFalse(has_active_conversation("inactiveone", connection_factory=factory))

    def test_db_error_returns_safe_default(self) -> None:
        conn = _make_conn()
        def failing_tx():
            return contextlib.nullcontext(_CompatConn(conn, fail=True))

        def failing_ro():
            return contextlib.nullcontext(_CompatConn(conn, fail=True))

        self.assertFalse(
            open_conversation(
                streamer_login="err",
                streamer_user_id="1",
                source="outreach",
                transaction_factory=failing_tx,
            )
        )
        self.assertFalse(
            append_message(
                streamer_login="err",
                role="voice",
                text="x",
                transaction_factory=failing_tx,
            )
        )
        self.assertFalse(
            update_state(
                streamer_login="err",
                new_state="brain_pending",
                transaction_factory=failing_tx,
            )
        )
        self.assertFalse(
            close_conversation(
                streamer_login="err",
                close_reason="error",
                transaction_factory=failing_tx,
            )
        )
        self.assertIsNone(
            get_conversation(streamer_login="err", connection_factory=failing_ro)
        )
        self.assertEqual(
            load_active_conversations(connection_factory=failing_ro),
            [],
        )
        self.assertFalse(has_active_conversation("err", connection_factory=failing_ro))

    def _seed_conversation(
        self,
        conn: sqlite3.Connection,
        streamer_login: str,
        *,
        state: str = "open",
        messages: list[dict] | None = None,
    ) -> None:
        conn.execute(
            "INSERT INTO twitch_partner_outreach (streamer_login, streamer_user_id, status) VALUES (?, ?, ?)",
            (streamer_login, f"{streamer_login}-id", "sent"),
        )
        conn.execute(
            """
            INSERT INTO twitch_partner_outreach_conversations
                (streamer_login, streamer_user_id, source, state, messages_json)
            VALUES (?, ?, ?, ?, ?)
            """,
            (
                streamer_login,
                f"{streamer_login}-id",
                "outreach",
                state,
                json.dumps(messages or [], separators=(",", ":")),
            ),
        )
        conn.commit()


if __name__ == "__main__":
    unittest.main()
