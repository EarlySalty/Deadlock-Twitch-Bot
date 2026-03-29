from __future__ import annotations

import unittest
from datetime import UTC, datetime
from unittest.mock import patch

from bot.analytics.api_public import _AnalyticsPublicMixin


class _FakeCursor:
    def __init__(self, rows=None, row=None) -> None:
        self._rows = list(rows or [])
        self._row = row

    def fetchall(self):
        return list(self._rows)

    def fetchone(self):
        return self._row


class _FakeConn:
    def __init__(self, *, bans=None, raids=None, stats_row=None) -> None:
        self._bans = list(bans or [])
        self._raids = list(raids or [])
        self._stats_row = stats_row

    def execute(self, sql: str, params=()):
        del params
        if "FROM twitch_ban_events" in sql and "COUNT(" not in sql:
            return _FakeCursor(rows=self._bans)
        if "FROM twitch_ban_events" in sql and "COUNT(" in sql:
            return _FakeCursor(row=self._stats_row)
        if "FROM twitch_raid_history" in sql:
            return _FakeCursor(rows=self._raids)
        raise AssertionError(f"Unexpected SQL: {sql}")


class _FakeConnCtx:
    def __init__(self, conn: _FakeConn) -> None:
        self._conn = conn

    def __enter__(self) -> _FakeConn:
        return self._conn

    def __exit__(self, exc_type, exc, tb) -> bool:
        return False


class _DummyPublicApi(_AnalyticsPublicMixin):
    pass


class AnalyticsPublicApiTests(unittest.TestCase):
    def test_recent_bans_accepts_text_received_at(self) -> None:
        handler = _DummyPublicApi()
        fake_conn = _FakeConn(
            bans=[("target_one", "mod_one", "spam", "2026-03-29T12:00:00+00:00")],
            stats_row=(2, 30, 5),
        )

        with patch(
            "bot.analytics.api_public.storage.readonly_connection",
            return_value=_FakeConnCtx(fake_conn),
        ):
            payload = handler._load_recent_bans_sync()

        self.assertEqual(
            payload["bans"][0],
            {
                "target_login": "target_one",
                "moderator_login": "mod_one",
                "reason": "spam",
                "received_at": "2026-03-29T12:00:00+00:00",
            },
        )
        self.assertEqual(payload["stats"], {"today": 2, "total_30d": 30, "channels_protected": 5})

    def test_recent_raids_keeps_datetime_serialization(self) -> None:
        handler = _DummyPublicApi()
        executed_at = datetime(2026, 3, 29, 12, 0, tzinfo=UTC)
        fake_conn = _FakeConn(raids=[("from_one", "to_one", 42, executed_at)])

        with patch(
            "bot.analytics.api_public.storage.readonly_connection",
            return_value=_FakeConnCtx(fake_conn),
        ):
            payload = handler._load_recent_raids_sync()

        self.assertEqual(
            payload["raids"][0],
            {
                "from_channel": "from_one",
                "to_channel": "to_one",
                "viewers": 42,
                "executed_at": executed_at.isoformat(),
            },
        )


if __name__ == "__main__":
    unittest.main()
