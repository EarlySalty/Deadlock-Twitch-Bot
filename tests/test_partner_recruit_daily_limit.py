"""Tests für das Tageslimit-Tracking im Partner-Recruit-Flow."""

import contextlib
import sqlite3
import unittest
from unittest.mock import patch

from bot.community.partner_recruit import TwitchPartnerRecruitMixin


class _Harness(TwitchPartnerRecruitMixin):
    pass


class _CompatConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def execute(self, sql: str, params=None):
        sql_text = str(sql or "").replace("%s", "?")
        sql_text = sql_text.replace(
            "contacted_at::timestamptz >= date_trunc('day', NOW())",
            "datetime(contacted_at) >= datetime('now', 'start of day')",
        )
        return self._conn.execute(sql_text, tuple(params or ()))

    def __getattr__(self, item):
        return getattr(self._conn, item)


def _make_conn() -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:")
    conn.row_factory = sqlite3.Row
    conn.execute(
        """
        CREATE TABLE twitch_partner_outreach (
            streamer_login TEXT PRIMARY KEY,
            contacted_at   TEXT,
            status         TEXT
        )
        """
    )
    return conn


class CountOutreachSentTodayTests(unittest.TestCase):
    def test_counts_only_sent_rows_today(self) -> None:
        conn = _make_conn()
        conn.execute(
            "INSERT INTO twitch_partner_outreach (streamer_login, contacted_at, status) "
            "VALUES (?, datetime('now', '-1 hours'), 'sent')",
            ("today_one",),
        )
        conn.execute(
            "INSERT INTO twitch_partner_outreach (streamer_login, contacted_at, status) "
            "VALUES (?, datetime('now', '-30 minutes'), 'sent')",
            ("today_two",),
        )
        conn.execute(
            "INSERT INTO twitch_partner_outreach (streamer_login, contacted_at, status) "
            "VALUES (?, datetime('now', '-2 hours'), 'failed')",
            ("today_failed",),
        )
        conn.execute(
            "INSERT INTO twitch_partner_outreach (streamer_login, contacted_at, status) "
            "VALUES (?, datetime('now', '-3 days'), 'sent')",
            ("yesterday_sent",),
        )
        conn.commit()

        with patch(
            "bot.community.partner_recruit.transaction",
            side_effect=lambda: contextlib.nullcontext(_CompatConn(conn)),
        ):
            count = _Harness()._count_outreach_sent_today()

        self.assertEqual(count, 2)
        conn.close()

    def test_returns_zero_on_db_error(self) -> None:
        def _raise():
            raise RuntimeError("db down")

        with patch(
            "bot.community.partner_recruit.transaction",
            side_effect=_raise,
        ):
            count = _Harness()._count_outreach_sent_today()

        self.assertEqual(count, 0)


if __name__ == "__main__":
    unittest.main()
