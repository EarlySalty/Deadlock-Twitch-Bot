"""Tests für den Outreach-Boost-Loader und die CAS-Markierung."""

import contextlib
import sqlite3
import unittest

from bot.raid.services.outreach_boost_targets import (
    load_outreach_boost_logins,
    mark_outreach_boost_used,
)


class _CompatConn:
    """Übersetzt PG-spezifisches SQL minimal nach SQLite."""

    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def execute(self, sql: str, params=None):
        sql_text = str(sql or "").replace("%s", "?")
        sql_text = sql_text.replace(
            "contacted_at::timestamptz >= NOW() - (? || ' hours')::interval",
            "datetime(contacted_at) >= datetime('now', '-' || ? || ' hours')",
        )
        sql_text = sql_text.replace("NOW()", "datetime('now')")
        return self._conn.execute(sql_text, tuple(params or ()))

    def commit(self):
        return self._conn.commit()

    def __getattr__(self, item):
        return getattr(self._conn, item)


def _make_conn() -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:")
    conn.row_factory = sqlite3.Row
    conn.execute(
        """
        CREATE TABLE twitch_partner_outreach (
            streamer_login   TEXT PRIMARY KEY,
            streamer_user_id TEXT,
            detected_at      TEXT,
            contacted_at     TEXT,
            status           TEXT,
            cooldown_until   TEXT,
            notes            TEXT,
            raid_used_at     TEXT
        )
        """
    )
    return conn


class OutreachBoostLoaderTests(unittest.TestCase):
    def test_returns_only_fresh_unused_sent_outreach(self) -> None:
        conn = _make_conn()
        conn.execute(
            "INSERT INTO twitch_partner_outreach "
            "(streamer_login, streamer_user_id, detected_at, contacted_at, status, raid_used_at) "
            "VALUES (?, ?, datetime('now'), datetime('now', '-1 hours'), 'sent', NULL)",
            ("freshboost", "111"),
        )
        conn.execute(
            "INSERT INTO twitch_partner_outreach "
            "(streamer_login, streamer_user_id, detected_at, contacted_at, status, raid_used_at) "
            "VALUES (?, ?, datetime('now'), datetime('now', '-72 hours'), 'sent', NULL)",
            ("staleboost", "222"),
        )
        conn.execute(
            "INSERT INTO twitch_partner_outreach "
            "(streamer_login, streamer_user_id, detected_at, contacted_at, status, raid_used_at) "
            "VALUES (?, ?, datetime('now'), datetime('now', '-2 hours'), 'sent', datetime('now'))",
            ("alreadyused", "333"),
        )
        conn.execute(
            "INSERT INTO twitch_partner_outreach "
            "(streamer_login, streamer_user_id, detected_at, contacted_at, status, raid_used_at) "
            "VALUES (?, ?, datetime('now'), datetime('now', '-2 hours'), 'failed', NULL)",
            ("notsent", "444"),
        )
        conn.commit()

        result = load_outreach_boost_logins(
            lookback_hours=48,
            connection_factory=lambda: contextlib.nullcontext(_CompatConn(conn)),
        )

        self.assertEqual(set(result.keys()), {"freshboost"})
        self.assertEqual(result["freshboost"]["streamer_user_id"], "111")

    def test_loader_returns_empty_dict_on_db_error(self) -> None:
        def _raising_factory():
            raise RuntimeError("db down")

        result = load_outreach_boost_logins(
            lookback_hours=48,
            connection_factory=_raising_factory,
        )
        self.assertEqual(result, {})


class OutreachBoostMarkUsedTests(unittest.TestCase):
    def test_marks_once_then_returns_false_on_repeat(self) -> None:
        conn = _make_conn()
        conn.execute(
            "INSERT INTO twitch_partner_outreach "
            "(streamer_login, streamer_user_id, detected_at, contacted_at, status, raid_used_at) "
            "VALUES (?, ?, datetime('now'), datetime('now', '-1 hours'), 'sent', NULL)",
            ("oneshot", "555"),
        )
        conn.commit()

        def factory():
            return contextlib.nullcontext(_CompatConn(conn))

        first = mark_outreach_boost_used("oneshot", transaction_factory=factory)
        second = mark_outreach_boost_used("oneshot", transaction_factory=factory)

        self.assertTrue(first)
        self.assertFalse(second)

        row = conn.execute(
            "SELECT raid_used_at FROM twitch_partner_outreach WHERE streamer_login = ?",
            ("oneshot",),
        ).fetchone()
        self.assertIsNotNone(row["raid_used_at"])

    def test_empty_login_returns_false(self) -> None:
        self.assertFalse(mark_outreach_boost_used(""))


if __name__ == "__main__":
    unittest.main()
