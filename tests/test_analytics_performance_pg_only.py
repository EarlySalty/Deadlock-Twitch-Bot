import contextlib
import json
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from bot.analytics.api_performance import _AnalyticsPerformanceMixin


class _FakeCursor:
    def __init__(self, rows):
        self._rows = list(rows)

    def fetchall(self):
        return list(self._rows)

    def fetchone(self):
        return self._rows[0] if self._rows else None


class _RecordingConnection:
    def __init__(self, responses):
        self._responses = list(responses)
        self.executed: list[tuple[str, tuple[object, ...]]] = []

    def execute(self, sql: str, params=()):
        self.executed.append((sql, tuple(params or ())))
        rows = self._responses.pop(0) if self._responses else []
        return _FakeCursor(rows)


class _PerformanceHarness(_AnalyticsPerformanceMixin):
    def _require_v2_auth(self, _request):
        return None

    def _require_extended_plan(self, _request):
        return None


class AnalyticsPerformancePgOnlyTests(unittest.IsolatedAsyncioTestCase):
    async def test_viewer_timeline_uses_pg_bucket_sql(self) -> None:
        conn = _RecordingConnection(
            [[("2026-03-20 10:00", 42.5, 55, 30, 3)]]
        )
        request = SimpleNamespace(query={"streamer": "Target", "days": "7"})

        with patch(
            "bot.storage.pg.readonly_connection",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            response = await _PerformanceHarness()._api_v2_viewer_timeline(request)

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertEqual(payload[0]["samples"], 3)
        executed_sql = conn.executed[0][0]
        self.assertIn("DATE_TRUNC('hour', ts_utc)", executed_sql)
        self.assertIn("FLOOR(EXTRACT(MINUTE FROM ts_utc) / 5)", executed_sql)
        self.assertNotIn("strftime", executed_sql.lower())
        self.assertNotIn("?", executed_sql)

    async def test_category_timings_uses_extract_for_hour_and_weekday(self) -> None:
        conn = _RecordingConnection(
            [[("streamer_one", 14, 2, 42)]]
        )
        request = SimpleNamespace(query={"days": "30", "source": "tracked"})

        with patch(
            "bot.storage.pg.readonly_connection",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            response = await _PerformanceHarness()._api_v2_category_timings(request)

        payload = json.loads(response.body.decode("utf-8"))
        hour_14 = next(item for item in payload["hourly"] if item["hour"] == 14)
        weekday_2 = next(item for item in payload["weekly"] if item["weekday"] == 2)
        self.assertEqual(response.status, 200)
        self.assertEqual(hour_14["median"], 42.0)
        self.assertEqual(weekday_2["median"], 42.0)
        executed_sql = conn.executed[0][0]
        self.assertIn("EXTRACT(HOUR FROM ts_utc)::integer", executed_sql)
        self.assertIn("EXTRACT(DOW FROM ts_utc)::integer", executed_sql)
        self.assertNotIn("strftime", executed_sql.lower())

    async def test_retention_curve_uses_pg_in_placeholders(self) -> None:
        conn = _RecordingConnection(
            [
                [(1, 100), (2, 120)],
                [(1, 0, 100), (1, 10, 80), (2, 0, 120), (2, 10, 72)],
                [],
            ]
        )
        request = SimpleNamespace(query={"streamer": "target", "days": "30"})

        with patch(
            "bot.storage.pg.readonly_connection",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            response = await _PerformanceHarness()._api_v2_retention_curve(request)

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertEqual(payload["sessions_used"], 2)
        self.assertEqual(payload["drop_events"][0]["minute"], 10)
        viewer_sql = conn.executed[1][0]
        ad_sql = conn.executed[2][0]
        self.assertIn("IN (%s,%s)", viewer_sql)
        self.assertIn("IN (%s,%s)", ad_sql)
        self.assertNotIn("?", viewer_sql)
        self.assertNotIn("?", ad_sql)


if __name__ == "__main__":
    unittest.main()
