import ast
import json
import os
import sqlite3
import unittest
from datetime import UTC, datetime, timedelta
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from bot.analytics.api_audience import _AnalyticsAudienceMixin
from bot.analytics.api_chat_deep import _AnalyticsChatDeepMixin
from bot.analytics.api_insights import _AnalyticsInsightsMixin
from bot.analytics.api_overview import _AnalyticsOverviewMixin
from bot.analytics.api_performance import _AnalyticsPerformanceMixin
from bot.analytics.chat_social_graph_loader import _MENTION_RE
from bot.analytics.api_raids import _AnalyticsRaidsMixin
from bot.analytics.coaching_engine import _schedule_optimizer
from bot.analytics.demo_data import get_audience_insights, get_overview
from bot.analytics.raid_metrics import raid_identity_key
from bot.analytics.api_viewers import _AnalyticsViewersMixin
from bot.analytics.api_v2 import AnalyticsV2Mixin, _compute_health_score


class _FakeCursor:
    def __init__(self, rows):
        self._rows = list(rows)

    def fetchall(self):
        return list(self._rows)

    def fetchone(self):
        return self._rows[0] if self._rows else None


class _ConnContext:
    def __init__(self, conn):
        if isinstance(conn, sqlite3.Connection):
            self._conn = _CompatSqliteConn(conn)
        else:
            self._conn = conn

    def __enter__(self):
        return self._conn

    def __exit__(self, exc_type, exc, tb):
        return False


class _RaidAnalyticsConn:
    def __init__(self, full_rows, sample_rows, follow_rows):
        self._full_rows = full_rows
        self._sample_rows = sample_rows
        self._follow_rows = follow_rows

    def execute(self, sql, params=None):
        if "FROM twitch_raid_retention rr" in sql and "JOIN twitch_raid_history rh" in sql:
            if "LIMIT 50" in sql:
                return _FakeCursor(self._sample_rows)
            return _FakeCursor(self._full_rows)
        if "FROM twitch_follow_events fe" in sql:
            return _FakeCursor(self._follow_rows)
        if "FROM twitch_raid_arrival_tracking rat" in sql:
            return _FakeCursor([])
        if "FROM twitch_stream_sessions ss" in sql and "ss.started_at <=" in sql:
            return _FakeCursor([])
        if "FROM twitch_session_viewers" in sql and "WHERE session_id = %s" in sql:
            return _FakeCursor([])
        if "COUNT(*) as follows" in sql and "FROM twitch_follow_events" in sql:
            return _FakeCursor([{"follows": 0}])
        raise AssertionError(f"Unexpected SQL in raid analytics test: {sql[:200]}")


class _CompatSqliteConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def execute(self, sql: str, params=None):
        sql_text = str(sql or "").replace("%s", "?")
        return self._conn.execute(sql_text, tuple(params or ()))

    def executemany(self, sql: str, params=None):
        sql_text = str(sql or "").replace("%s", "?")
        return self._conn.executemany(sql_text, params or ())

    def __getattr__(self, item):
        return getattr(self._conn, item)


class _AudienceDemographicsConn:
    def execute(self, sql, params=None):
        if "GROUP BY lang" in sql:
            return _FakeCursor([("en", 3, 22.0)])
        if "GROUP BY hour" in sql:
            return _FakeCursor([])
        if "viewer_minutes_fallback" in sql:
            return _FakeCursor([(3, 7200, 18.0, 2160.0)])
        if "FROM twitch_session_viewers sv" in sql:
            return _FakeCursor([(10, 1800.0)])
        if "WITH per_user AS" in sql:
            return _FakeCursor([])
        if "FROM twitch_chat_messages cm" in sql and "SELECT COUNT(*)" in sql:
            return _FakeCursor([(0,)])
        if "COUNT(DISTINCT sc.session_id)" in sql and "FROM twitch_session_chatters sc" in sql:
            return _FakeCursor([(0,)])
        if "GROUP BY weekday" in sql:
            return _FakeCursor([(1, 2), (6, 1)])
        raise AssertionError(f"Unexpected SQL in audience demographics test: {sql[:200]}")


class _AudienceInsightsConn:
    def execute(self, sql, params=None):
        sql_text = str(sql)
        if (
            "SELECT s.id, s.retention_5m, s.retention_10m," in sql_text
            and "s.started_at >= %s AND LOWER(s.streamer_login) = %s" in sql_text
            and "s.started_at < %s" not in sql_text
        ):
            return _FakeCursor(
                [
                    (101, 0.4, 0.5, 0.6, 20.0, 10, 18, 3600),
                    (102, 0.45, 0.55, 0.65, 24.0, 12, 20, 4200),
                ]
            )
        if (
            "SELECT s.id, s.retention_5m, s.retention_10m," in sql_text
            and "s.started_at >= %s AND s.started_at < %s" in sql_text
        ):
            return _FakeCursor(
                [
                    (91, 0.35, 0.4, 0.5, 18.0, 9, 16, 3300),
                    (92, 0.38, 0.42, 0.52, 19.0, 10, 17, 3500),
                ]
            )
        if "AVG(s.retention_10m) as curr_ret" in sql_text:
            return _FakeCursor([(0.55, 0.44)])
        if "COUNT(DISTINCT pv.viewer_key) AS total_viewers" in sql_text:
            if "s.started_at < %s" in sql_text:
                return _FakeCursor([(8, 4)])
            return _FakeCursor([(10, 6)])
        raise AssertionError(f"Unexpected SQL in audience insights test: {sql[:200]}")


class _AudienceInsightsNoBaselineConn(_AudienceInsightsConn):
    def execute(self, sql, params=None):
        sql_text = str(sql)
        if "COUNT(DISTINCT pv.viewer_key) AS total_viewers" in sql_text and "s.started_at < %s" in sql_text:
            return _FakeCursor([(0, 0)])
        return super().execute(sql, params)


class _OverviewRaidRetentionConn:
    def __init__(self, rows):
        self._rows = rows

    def execute(self, sql, params=None):
        if "FROM twitch_raid_retention" in sql:
            return _FakeCursor(self._rows)
        raise AssertionError(f"Unexpected SQL in overview raid retention test: {sql[:200]}")


class _PlaceholderParityCursor:
    def __init__(self, rows=None):
        self._rows = list(rows or [])

    def fetchall(self):
        return list(self._rows)

    def fetchone(self):
        return self._rows[0] if self._rows else None


class _OverviewPlaceholderParityConn:
    def execute(self, sql, params=None):
        params = list(params or [])
        placeholder_count = str(sql).count("%s")
        if placeholder_count != len(params):
            raise AssertionError(
                f"Expected placeholder/param parity, got {placeholder_count} placeholders and {len(params)} params"
            )
        if "?" in str(sql):
            raise AssertionError("Unexpected sqlite-style '?' placeholder in psycopg query")

        sql_text = str(sql)
        if "ORDER BY s.started_at DESC" in sql_text and "LIMIT %s" in sql_text:
            return _PlaceholderParityCursor([])
        if "AVG(s.avg_viewers) as avg_avg_viewers" in sql_text:
            return _PlaceholderParityCursor([(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)])
        if "SUM(CASE WHEN s.follower_delta > 0" in sql_text:
            return _PlaceholderParityCursor([(0,)])
        if "FROM twitch_chatter_rollup" in sql_text:
            return _PlaceholderParityCursor([(0,)])
        if "COUNT(CASE" in sql_text and "retention_10m" in sql_text:
            return _PlaceholderParityCursor([(0, 0, 0)])
        if "SELECT COUNT(DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id))" in sql_text:
            return _PlaceholderParityCursor([(0,)])
        raise AssertionError(f"Unexpected SQL in placeholder parity test: {sql[:200]}")


class _ChatAnalyticsSqlGuardConn:
    def __init__(self):
        self.checked_first_time_cast = False
        self.checked_top_chatter_identity = False

    def execute(self, sql, params=None):
        if "viewer_minutes_fallback" in sql and "FROM twitch_stream_sessions s" in sql:
            return _FakeCursor([(1, 3600, 12.0, 720.0)])
        if "FROM twitch_session_viewers sv" in sql:
            return _FakeCursor([(0, 0)])
        if (
            "FROM twitch_chat_messages" in sql
            and "SELECT message_ts, content, is_command, chatter_login, chatter_id" in sql
        ):
            return _FakeCursor([])
        if "WITH per_user AS" in sql and "FROM twitch_session_chatters sc" in sql:
            if "sc.is_first_time_streamer IS TRUE" in sql:
                raise AssertionError("Expected cast-based first-time flag expression, found IS TRUE")
            if "CAST(sc.is_first_time_streamer AS TEXT)" not in sql:
                raise AssertionError("Missing cast-based first-time flag expression")
            self.checked_first_time_cast = True
            return _FakeCursor([])
        if "SELECT COUNT(DISTINCT sc.session_id)" in sql and "FROM twitch_session_chatters sc" in sql:
            return _FakeCursor([(0,)])
        if "FROM twitch_chat_messages cm" in sql:
            if "COALESCE(NULLIF(cm.chatter_login, ''), cm.chatter_id, 'unknown')" in sql:
                raise AssertionError("Top chatter query should not synthesize an 'unknown' chatter key")
            if "COALESCE(NULLIF(cm.chatter_login, ''), cm.chatter_id) IS NOT NULL" not in sql:
                raise AssertionError("Top chatter query must exclude rows without any chatter identity")
            self.checked_top_chatter_identity = True
            return _FakeCursor([])
        raise AssertionError(f"Unexpected SQL in chat analytics SQL guard test: {sql[:200]}")


class _DummyRaids(_AnalyticsRaidsMixin):
    def _require_v2_auth(self, request):
        return None

    def _require_extended_plan(self, request):
        return None


class _DummyAudience(_AnalyticsAudienceMixin):
    def _require_v2_auth(self, request):
        return None

    def _require_extended_plan(self, request):
        return None


class _DummyInsights(_AnalyticsInsightsMixin):
    def _require_v2_auth(self, request):
        return None

    def _require_extended_plan(self, request):
        return None


class _DummyPerformance(_AnalyticsPerformanceMixin):
    def _require_v2_auth(self, request):
        return None

    def _require_extended_plan(self, request):
        return None


class _DummyChatDeep(_AnalyticsChatDeepMixin):
    def _require_v2_auth(self, request):
        return None

    def _require_extended_plan(self, request):
        return None


class _DummyOverview(_AnalyticsOverviewMixin):
    def _require_v2_auth(self, request):
        return None

    def _require_extended_plan(self, request):
        return None


class _DummyViewers(_AnalyticsViewersMixin):
    def _require_v2_auth(self, request):
        return None

    def _require_extended_plan(self, request):
        return None


class _DummyV2(AnalyticsV2Mixin):
    def _require_v2_auth(self, request):
        return None

    def _require_extended_plan(self, request):
        return None


class _ChatHypeTimelineConn:
    def execute(self, sql, params=None):
        if "FROM twitch_stream_sessions WHERE id = %s" in sql:
            return _FakeCursor(
                [
                    (
                        1,
                        "target",
                        datetime(2026, 2, 1, 12, 0, tzinfo=UTC),
                        3600,
                        "Hype Session",
                    )
                ]
            )
        if "time_bucket('1 minute', m.message_ts) AS bucket" in sql:
            return _FakeCursor(
                [
                    (
                        datetime(2026, 2, 1, 12, 0, tzinfo=UTC),
                        8,
                        3,
                    ),
                    (
                        datetime(2026, 2, 1, 12, 1, tzinfo=UTC),
                        4,
                        2,
                    ),
                ]
            )
        if "FROM twitch_session_viewers" in sql:
            return _FakeCursor([(None, 77), (0, 42), ("1", 43)])
        if "FROM twitch_stream_sessions s" in sql and "LIMIT 10" in sql:
            return _FakeCursor([])
        raise AssertionError(f"Unexpected SQL in chat hype timeline test: {sql[:200]}")


class RaidAnalyticsRegressionTests(unittest.IsolatedAsyncioTestCase):
    async def test_full_window_per_source_and_sample_retention_curves(self) -> None:
        full_rows = []
        for i in range(60):
            source = "raider_a" if i < 55 else "raider_b"
            full_rows.append(
                {
                    "raid_id": i + 1,
                    "from_broadcaster_login": source,
                    "viewer_count_sent": 100,
                    "executed_at": "2026-02-01T12:00:00+00:00",
                    "target_session_id": 1000 + i,
                    "to_broadcaster_login": "target",
                }
            )
        sample_rows = full_rows[:50]
        follow_rows = (
            [{"follow_source": "raid", "raid_source": "raider_a"} for _ in range(11)]
            + [{"follow_source": "raid", "raid_source": "raider_b"} for _ in range(2)]
            + [{"follow_source": "organic", "raid_source": None} for _ in range(3)]
        )

        def _fake_metrics(_conn, raids):
            return {
                raid_identity_key(raid["raid_id"], raid["executed_at"]): {
                    "plus5m": 10,
                    "plus15m": 20,
                    "plus30m": 50,
                    "known_from_raider": 5,
                    "new_chatters": 8,
                }
                for raid in raids
            }

        handler = _DummyRaids()
        request = SimpleNamespace(query={"streamer": "target", "days": "90"})
        with (
            patch(
                "bot.analytics.api_raids.storage.readonly_connection",
                return_value=_ConnContext(_RaidAnalyticsConn(full_rows, sample_rows, follow_rows)),
            ),
            patch(
                "bot.analytics.api_raids.recalculate_raid_chat_metrics",
                side_effect=_fake_metrics,
            ) as metrics_mock,
        ):
            response = await handler._api_v2_raid_analytics(request)

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertEqual(metrics_mock.call_count, 1)
        self.assertEqual(len(metrics_mock.call_args.args[1]), 60)
        self.assertEqual(len(payload["retention_curves"]), 50)
        self.assertEqual(payload["per_source"][0]["from_channel"], "raider_a")
        self.assertEqual(payload["per_source"][0]["raids_received"], 55)
        self.assertEqual(
            payload["per_source"][0]["conversion_rate"],
            round(11 / (55 * 100), 3),
        )
        self.assertEqual(payload["dataQuality"]["retentionCurveSampleSize"], 50)
        self.assertTrue(payload["dataQuality"]["perSourceUsesFullWindow"])

    async def test_zero_viewer_raids_are_kept_for_consistent_outputs(self) -> None:
        full_rows = [
            {
                "raid_id": 1,
                "from_broadcaster_login": "raider_a",
                "viewer_count_sent": 0,
                "executed_at": "2026-02-01T12:00:00+00:00",
                "target_session_id": 1000,
                "to_broadcaster_login": "target",
            }
        ]
        sample_rows = list(full_rows)
        follow_rows = [{"follow_source": "raid", "raid_source": "raider_a"}]

        def _fake_metrics(_conn, raids):
            return {
                raid_identity_key(raid["raid_id"], raid["executed_at"]): {
                    "plus5m": 0,
                    "plus15m": 0,
                    "plus30m": 0,
                    "known_from_raider": 0,
                    "new_chatters": 0,
                }
                for raid in raids
            }

        handler = _DummyRaids()
        request = SimpleNamespace(query={"streamer": "target", "days": "90"})
        with (
            patch(
                "bot.analytics.api_raids.storage.readonly_connection",
                return_value=_ConnContext(_RaidAnalyticsConn(full_rows, sample_rows, follow_rows)),
            ),
            patch(
                "bot.analytics.api_raids.recalculate_raid_chat_metrics",
                side_effect=_fake_metrics,
            ),
        ):
            response = await handler._api_v2_raid_analytics(request)

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertEqual(len(payload["per_source"]), 1)
        self.assertEqual(payload["per_source"][0]["from_channel"], "raider_a")
        self.assertEqual(payload["per_source"][0]["raids_received"], 1)
        self.assertEqual(len(payload["retention_curves"]), 1)
        self.assertEqual(payload["retention_curves"][0]["viewers_sent"], 0)
        self.assertEqual(payload["retention_curves"][0]["retention_curve"]["plus30m"], 0.0)
        self.assertIsNone(payload["per_source"][0]["avg_retention_30m"])
        self.assertIsNone(payload["per_source"][0]["known_audience_overlap"])

    async def test_zero_viewer_raids_do_not_dilute_ratio_averages(self) -> None:
        full_rows = [
            {
                "raid_id": 1,
                "from_broadcaster_login": "raider_a",
                "viewer_count_sent": 100,
                "executed_at": "2026-02-01T12:00:00+00:00",
                "target_session_id": 1000,
                "to_broadcaster_login": "target",
            },
            {
                "raid_id": 2,
                "from_broadcaster_login": "raider_a",
                "viewer_count_sent": 0,
                "executed_at": "2026-02-02T12:00:00+00:00",
                "target_session_id": 1001,
                "to_broadcaster_login": "target",
            },
        ]
        sample_rows = list(full_rows)
        follow_rows = []

        def _fake_metrics(_conn, raids):
            result = {}
            for raid in raids:
                raid_id = int(raid["raid_id"])
                raid_key = raid_identity_key(raid["raid_id"], raid["executed_at"])
                assert raid_key is not None
                if raid_id == 1:
                    result[raid_key] = {
                        "plus5m": 20,
                        "plus15m": 35,
                        "plus30m": 50,
                        "known_from_raider": 20,
                        "new_chatters": 12,
                    }
                else:
                    result[raid_key] = {
                        "plus5m": 0,
                        "plus15m": 0,
                        "plus30m": 0,
                        "known_from_raider": 0,
                        "new_chatters": 0,
                    }
            return result

        handler = _DummyRaids()
        request = SimpleNamespace(query={"streamer": "target", "days": "90"})
        with (
            patch(
                "bot.analytics.api_raids.storage.readonly_connection",
                return_value=_ConnContext(_RaidAnalyticsConn(full_rows, sample_rows, follow_rows)),
            ),
            patch(
                "bot.analytics.api_raids.recalculate_raid_chat_metrics",
                side_effect=_fake_metrics,
            ),
        ):
            response = await handler._api_v2_raid_analytics(request)

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertEqual(len(payload["per_source"]), 1)
        self.assertEqual(payload["per_source"][0]["from_channel"], "raider_a")
        self.assertEqual(payload["per_source"][0]["avg_retention_30m"], 0.5)
        self.assertEqual(payload["per_source"][0]["known_audience_overlap"], 0.2)

    async def test_duplicate_raid_ids_keep_retention_curves_split_by_executed_at(self) -> None:
        full_rows = [
            {
                "raid_id": 7,
                "from_broadcaster_login": "raider_a",
                "viewer_count_sent": 10,
                "executed_at": "2026-02-02T12:00:00+00:00",
                "target_session_id": 1001,
                "to_broadcaster_login": "target",
            },
            {
                "raid_id": 7,
                "from_broadcaster_login": "raider_a",
                "viewer_count_sent": 10,
                "executed_at": "2026-02-01T12:00:00+00:00",
                "target_session_id": 1000,
                "to_broadcaster_login": "target",
            },
        ]

        def _fake_metrics(_conn, _raids):
            return {
                raid_identity_key(7, "2026-02-02T12:00:00+00:00"): {
                    "plus5m": 2,
                    "plus15m": 4,
                    "plus30m": 8,
                    "known_from_raider": 1,
                    "new_chatters": 5,
                },
                raid_identity_key(7, "2026-02-01T12:00:00+00:00"): {
                    "plus5m": 1,
                    "plus15m": 2,
                    "plus30m": 6,
                    "known_from_raider": 3,
                    "new_chatters": 2,
                },
            }

        handler = _DummyRaids()
        request = SimpleNamespace(query={"streamer": "target", "days": "90"})
        with (
            patch(
                "bot.analytics.api_raids.storage.readonly_connection",
                return_value=_ConnContext(_RaidAnalyticsConn(full_rows, list(full_rows), [])),
            ),
            patch(
                "bot.analytics.api_raids.recalculate_raid_chat_metrics",
                side_effect=_fake_metrics,
            ),
        ):
            response = await handler._api_v2_raid_analytics(request)

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertEqual(len(payload["retention_curves"]), 2)
        self.assertEqual(payload["retention_curves"][0]["raid_id"], 7)
        self.assertEqual(payload["retention_curves"][0]["new_chatters"], 5)
        self.assertEqual(payload["retention_curves"][0]["retention_curve"]["plus30m"], 0.8)
        self.assertEqual(payload["retention_curves"][1]["raid_id"], 7)
        self.assertEqual(payload["retention_curves"][1]["new_chatters"], 2)
        self.assertEqual(payload["retention_curves"][1]["retention_curve"]["plus30m"], 0.6)

    async def test_raid_analytics_recalculates_in_batches_for_large_windows(self) -> None:
        total_raids = 1201
        full_rows = [
            {
                "raid_id": i + 1,
                "from_broadcaster_login": "raider_big",
                "viewer_count_sent": 10,
                "executed_at": "2026-02-01T12:00:00+00:00",
                "target_session_id": 5000 + i,
                "to_broadcaster_login": "target",
            }
            for i in range(total_raids)
        ]
        follow_rows = []
        batch_sizes = []

        def _fake_metrics(_conn, raids):
            batch_sizes.append(len(raids))
            return {
                raid_identity_key(raid["raid_id"], raid["executed_at"]): {
                    "plus5m": 1,
                    "plus15m": 2,
                    "plus30m": 3,
                    "known_from_raider": 1,
                    "new_chatters": 1,
                }
                for raid in raids
            }

        handler = _DummyRaids()
        request = SimpleNamespace(query={"streamer": "target", "days": "365"})
        with (
            patch(
                "bot.analytics.api_raids.storage.readonly_connection",
                return_value=_ConnContext(_RaidAnalyticsConn(full_rows, [], follow_rows)),
            ),
            patch(
                "bot.analytics.api_raids.recalculate_raid_chat_metrics",
                side_effect=_fake_metrics,
            ),
        ):
            response = await handler._api_v2_raid_analytics(request)

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertTrue(batch_sizes)
        self.assertEqual(sum(batch_sizes), total_raids)
        self.assertGreater(len(batch_sizes), 1)
        self.assertTrue(all(size <= handler.RAID_METRIC_BATCH_SIZE for size in batch_sizes))
        self.assertEqual(payload["dataQuality"]["raidMetricBatchSize"], handler.RAID_METRIC_BATCH_SIZE)


class AudienceDemographicsRegressionTests(unittest.IsolatedAsyncioTestCase):
    async def test_demographics_endpoint_no_runtime_nameerror(self) -> None:
        handler = _DummyAudience()
        request = SimpleNamespace(query={"streamer": "target", "days": "30"})
        with (
            patch(
                "bot.analytics.api_audience.storage.transaction",
                return_value=_ConnContext(_AudienceDemographicsConn()),
            ),
            patch.object(
                _DummyAudience,
                "_compute_weighted_peak_hours",
                return_value=(
                    [],
                    {
                        "sessionCount": 0,
                        "sessionsWithActivity": 0,
                        "sampleCount": 0,
                        "coverage": 0.0,
                    },
                ),
            ),
        ):
            response = await handler._api_v2_audience_demographics(request)

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertIn("dataQuality", payload)
        self.assertTrue(payload["dataQuality"]["botFilterApplied"])


class AudienceInsightsRegressionTests(unittest.IsolatedAsyncioTestCase):
    async def test_audience_insights_uses_watch_time_delta_and_exposes_top_level_contract_fields(
        self,
    ) -> None:
        handler = _DummyAudience()
        request = SimpleNamespace(query={"streamer": "target", "days": "30"})

        def _fake_watch_distribution(_sessions, conn=None, session_ids=None):
            if list(session_ids or []) == [101, 102]:
                return {
                    "avgWatchTime": 30.0,
                    "dataQuality": {"method": "real_samples"},
                }
            if list(session_ids or []) == [91, 92]:
                return {
                    "avgWatchTime": 20.0,
                    "dataQuality": {"method": "real_samples"},
                }
            raise AssertionError(f"Unexpected session_ids for watch distribution: {session_ids}")

        with (
            patch(
                "bot.analytics.api_audience.storage.transaction",
                return_value=_ConnContext(_AudienceInsightsConn()),
            ),
            patch.object(
                _DummyAudience,
                "_backfill_last_seen_from_messages",
                return_value=0,
            ),
            patch.object(
                _DummyAudience,
                "_calc_watch_distribution",
                side_effect=_fake_watch_distribution,
            ),
        ):
            response = await handler._api_v2_audience_insights(request)

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertEqual(payload["trends"]["watchTimeChange"], 50.0)
        self.assertIsNone(payload["trends"]["conversionChange"])
        self.assertEqual(payload["trends"]["viewerReturnRate"], 60.0)
        self.assertEqual(payload["trends"]["viewerReturnChange"], 20.0)
        self.assertEqual(payload["distinctViewers"], 10)
        self.assertEqual(payload["returnRateMethod"], "distinct_rollup")
        self.assertEqual(payload["dataQuality"]["watchTimeMethod"], "real_samples")
        self.assertTrue(payload["dataQuality"]["watchTimeTrendAvailable"])
        self.assertTrue(payload["dataQuality"]["viewerReturnTrendAvailable"])
        self.assertFalse(payload["dataQuality"]["conversionTrendAvailable"])

    async def test_audience_insights_returns_null_watch_time_change_without_real_baseline(
        self,
    ) -> None:
        handler = _DummyAudience()
        request = SimpleNamespace(query={"streamer": "target", "days": "30"})

        def _fake_watch_distribution(_sessions, conn=None, session_ids=None):
            if list(session_ids or []) == [101, 102]:
                return {
                    "avgWatchTime": 30.0,
                    "dataQuality": {"method": "real_samples"},
                }
            if list(session_ids or []) == [91, 92]:
                return {
                    "avgWatchTime": 0.0,
                    "dataQuality": {"method": "low_coverage"},
                }
            raise AssertionError(f"Unexpected session_ids for watch distribution: {session_ids}")

        with (
            patch(
                "bot.analytics.api_audience.storage.transaction",
                return_value=_ConnContext(_AudienceInsightsConn()),
            ),
            patch.object(
                _DummyAudience,
                "_backfill_last_seen_from_messages",
                return_value=0,
            ),
            patch.object(
                _DummyAudience,
                "_calc_watch_distribution",
                side_effect=_fake_watch_distribution,
            ),
        ):
            response = await handler._api_v2_audience_insights(request)

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertIsNone(payload["trends"]["watchTimeChange"])
        self.assertFalse(payload["dataQuality"]["watchTimeTrendAvailable"])
        self.assertTrue(payload["dataQuality"]["viewerReturnTrendAvailable"])
        self.assertFalse(payload["dataQuality"]["conversionTrendAvailable"])

    async def test_audience_insights_returns_null_viewer_return_change_without_previous_baseline(
        self,
    ) -> None:
        handler = _DummyAudience()
        request = SimpleNamespace(query={"streamer": "target", "days": "30"})

        def _fake_watch_distribution(_sessions, conn=None, session_ids=None):
            if list(session_ids or []) in ([101, 102], [91, 92]):
                return {
                    "avgWatchTime": 20.0,
                    "dataQuality": {"method": "real_samples"},
                }
            raise AssertionError(f"Unexpected session_ids for watch distribution: {session_ids}")

        with (
            patch(
                "bot.analytics.api_audience.storage.transaction",
                return_value=_ConnContext(_AudienceInsightsNoBaselineConn()),
            ),
            patch.object(
                _DummyAudience,
                "_backfill_last_seen_from_messages",
                return_value=0,
            ),
            patch.object(
                _DummyAudience,
                "_calc_watch_distribution",
                side_effect=_fake_watch_distribution,
            ),
        ):
            response = await handler._api_v2_audience_insights(request)

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertIsNone(payload["trends"]["viewerReturnChange"])
        self.assertFalse(payload["dataQuality"]["viewerReturnTrendAvailable"])
        self.assertFalse(payload["dataQuality"]["conversionTrendAvailable"])


class DemoAudienceInsightsRegressionTests(unittest.TestCase):
    def test_demo_audience_insights_matches_live_contract(self) -> None:
        payload = get_audience_insights()
        self.assertNotIn("watchTimeDistribution", payload)
        self.assertNotIn("followerFunnel", payload)
        self.assertNotIn("tagPerformance", payload)
        self.assertNotIn("titlePerformance", payload)
        self.assertIsNone(payload["trends"]["conversionChange"])
        self.assertEqual(payload["returnRateMethod"], "distinct_rollup")
        self.assertTrue(payload["dataQuality"]["watchTimeTrendAvailable"])
        self.assertTrue(payload["dataQuality"]["viewerReturnTrendAvailable"])
        self.assertFalse(payload["dataQuality"]["conversionTrendAvailable"])

    def test_demo_overview_embeds_live_audience_insights_contract(self) -> None:
        payload = get_overview()
        audience_insights = payload["audienceInsights"]
        self.assertNotIn("watchTimeDistribution", audience_insights)
        self.assertNotIn("followerFunnel", audience_insights)
        self.assertIsNone(audience_insights["trends"]["conversionChange"])
        self.assertEqual(audience_insights["returnRateMethod"], "distinct_rollup")
        self.assertTrue(audience_insights["dataQuality"]["watchTimeTrendAvailable"])
        self.assertTrue(audience_insights["dataQuality"]["viewerReturnTrendAvailable"])


class InsightsSqlRegressionTests(unittest.IsolatedAsyncioTestCase):
    async def test_chat_analytics_uses_cast_based_first_time_expression(self) -> None:
        handler = _DummyInsights()
        request = SimpleNamespace(query={"streamer": "target", "days": "30", "timezone": "UTC"})
        conn = _ChatAnalyticsSqlGuardConn()
        with (
            patch(
                "bot.analytics.api_insights.storage.readonly_connection",
                return_value=_ConnContext(conn),
            ),
            patch(
                "bot.analytics.api_insights.build_raw_chat_status",
                return_value={
                    "available": False,
                    "lastMessageAt": None,
                    "gapStart": None,
                    "suspectedIngestionIssue": False,
                    "backfillState": "not_needed",
                    "note": None,
                },
            ),
        ):
            response = await handler._api_v2_chat_analytics(request)

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertTrue(conn.checked_first_time_cast)
        self.assertTrue(conn.checked_top_chatter_identity)
        self.assertTrue(payload["dataQuality"]["botFilterApplied"])


class AnalyticsOffloadingRegressionTests(unittest.IsolatedAsyncioTestCase):
    async def test_tag_analysis_extended_offloads_db_loader(self) -> None:
        handler = _DummyPerformance()
        request = SimpleNamespace(query={"streamer": "target", "days": "30", "limit": "20"})
        to_thread_calls = []

        async def _fake_to_thread(func, /, *args, **kwargs):
            to_thread_calls.append((func, args, kwargs))
            return func(*args, **kwargs)

        with (
            patch.object(
                handler,
                "_load_tag_analysis_extended_payload_sync",
                return_value={"tags": [], "peerBenchmark": None},
            ) as loader,
            patch("bot.analytics.api_performance.asyncio.to_thread", side_effect=_fake_to_thread),
        ):
            response = await handler._api_v2_tag_analysis_extended(request)

        self.assertEqual(response.status, 200)
        loader.assert_called_once_with(streamer="target", days=30, limit=20)
        self.assertEqual(len(to_thread_calls), 1)

    async def test_chat_analytics_offloads_db_snapshot_loader(self) -> None:
        handler = _DummyInsights()
        request = SimpleNamespace(query={"streamer": "target", "days": "30", "timezone": "UTC"})
        snapshot = {
            "session_stats": (1, 3600, 12.0, 720.0),
            "viewer_sample_row": (0, 0),
            "session_benchmark_rows": [],
            "all_messages": [],
            "chatter_rows": [],
            "sessions_with_chat_row": (0,),
            "top_chatters": [],
            "raw_chat_status": {
                "available": False,
                "lastMessageAt": None,
                "gapStart": None,
                "suspectedIngestionIssue": False,
                "backfillState": "not_needed",
                "note": None,
            },
        }
        to_thread_calls = []

        async def _fake_to_thread(func, /, *args, **kwargs):
            to_thread_calls.append((func, args, kwargs))
            return func(*args, **kwargs)

        with (
            patch.object(handler, "_load_chat_analytics_snapshot_sync", return_value=snapshot) as loader,
            patch("bot.analytics.api_insights.asyncio.to_thread", side_effect=_fake_to_thread),
        ):
            response = await handler._api_v2_chat_analytics(request)

        self.assertEqual(response.status, 200)
        loader.assert_called_once()
        self.assertEqual(loader.call_args.kwargs["streamer_login"], "target")
        self.assertEqual(len(to_thread_calls), 1)

    async def test_chat_content_analysis_offloads_db_loader(self) -> None:
        handler = _DummyChatDeep()
        request = SimpleNamespace(query={"streamer": "target", "days": "30"})
        to_thread_calls = []

        async def _fake_to_thread(func, /, *args, **kwargs):
            to_thread_calls.append((func, args, kwargs))
            return func(*args, **kwargs)

        with (
            patch.object(
                handler,
                "_load_chat_content_analysis_payload_sync",
                return_value={
                    "heroMentions": [],
                    "topicBreakdown": {},
                    "sentimentTimeline": [],
                    "overallSentiment": {
                        "score": 0,
                        "label": "neutral",
                        "trend": "insufficient_data",
                        "totalAnalyzed": 0,
                        "positiveCount": 0,
                        "negativeCount": 0,
                    },
                    "backseat": {"count": 0, "pct": 0, "examples": []},
                    "engagementDepth": {
                        "reaction": 0,
                        "reactionPct": 0,
                        "short": 0,
                        "shortPct": 0,
                        "discussion": 0,
                        "discussionPct": 0,
                        "total": 0,
                        "avgWordCount": 0,
                    },
                    "rawChatStatus": None,
                },
            ) as loader,
            patch("bot.analytics.api_chat_deep.asyncio.to_thread", side_effect=_fake_to_thread),
        ):
            response = await handler._api_v2_chat_content_analysis(request)

        self.assertEqual(response.status, 200)
        loader.assert_called_once_with(streamer="target", days=30)
        self.assertEqual(len(to_thread_calls), 1)


class ViewerDirectoryGapRegressionTests(unittest.IsolatedAsyncioTestCase):
    @staticmethod
    def _setup_tables(conn: sqlite3.Connection) -> None:
        conn.execute(
            """
            CREATE TABLE twitch_chatter_rollup (
                streamer_login TEXT,
                chatter_login TEXT,
                total_sessions INTEGER,
                total_messages INTEGER,
                first_seen_at TEXT,
                last_seen_at TEXT
            )
            """
        )
        conn.execute(
            """
            CREATE TABLE twitch_stream_sessions (
                id INTEGER PRIMARY KEY,
                streamer_login TEXT,
                started_at TEXT,
                ended_at TEXT
            )
            """
        )
        conn.execute(
            """
            CREATE TABLE twitch_session_chatters (
                session_id INTEGER,
                streamer_login TEXT,
                chatter_login TEXT,
                messages INTEGER
            )
            """
        )
        conn.execute(
            """
            CREATE TABLE twitch_chat_messages (
                session_id INTEGER,
                streamer_login TEXT,
                chatter_login TEXT,
                message_ts TEXT,
                content TEXT
            )
            """
        )
        conn.execute(
            """
            CREATE TABLE twitch_raw_chat_ingest_health (
                streamer_login TEXT,
                last_raw_chat_message_at TEXT,
                last_raw_chat_insert_ok_at TEXT,
                last_raw_chat_insert_error_at TEXT,
                last_raw_chat_error TEXT
            )
            """
        )

    async def test_viewer_directory_flags_presence_only_gap_with_raw_chat_status(self) -> None:
        conn = sqlite3.connect(":memory:", check_same_thread=False)
        self._setup_tables(conn)
        recent_started = datetime.now(UTC) - timedelta(days=5)
        recent_ended = recent_started + timedelta(hours=2)
        conn.execute(
            """
            INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at)
            VALUES (?, ?, ?, ?)
            """,
            (
                1,
                "target",
                recent_started.isoformat(),
                recent_ended.isoformat(),
            ),
        )
        conn.execute(
            """
            INSERT INTO twitch_session_chatters (
                session_id, streamer_login, chatter_login, messages
            ) VALUES (?, ?, ?, ?)
            """,
            (1, "target", "purebacon_", 0),
        )
        conn.execute(
            """
            INSERT INTO twitch_chatter_rollup (
                streamer_login, chatter_login, total_sessions, total_messages, first_seen_at, last_seen_at
            ) VALUES (?, ?, ?, ?, ?, ?)
            """,
            (
                "target",
                "purebacon_",
                1,
                0,
                recent_started.isoformat(),
                recent_started.isoformat(),
            ),
        )
        conn.execute(
            """
            INSERT INTO twitch_raw_chat_ingest_health (
                streamer_login, last_raw_chat_message_at, last_raw_chat_insert_ok_at,
                last_raw_chat_insert_error_at, last_raw_chat_error
            ) VALUES (?, ?, ?, ?, ?)
            """,
            (
                "target",
                (recent_started + timedelta(minutes=5)).isoformat(),
                None,
                (recent_started + timedelta(minutes=5)).isoformat(),
                "insert timeout",
            ),
        )
        conn.commit()

        handler = _DummyViewers()
        request = SimpleNamespace(query={"streamer": "target", "days": "30"})
        try:
            with patch("bot.analytics.api_viewers.storage.readonly_connection", return_value=_ConnContext(conn)):
                response = await handler._api_v2_viewer_directory(request)
        finally:
            conn.close()

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertTrue(payload["rawChatStatus"]["suspectedIngestionIssue"])
        self.assertEqual(payload["viewers"][0]["login"], "purebacon_")
        self.assertTrue(payload["viewers"][0]["presenceOnlyInWindow"])
        self.assertFalse(payload["viewers"][0]["hasRawMessages"])
        self.assertTrue(payload["viewers"][0]["messageGapNote"])

    async def test_viewer_directory_uses_selected_window_for_core_counts(self) -> None:
        conn = sqlite3.connect(":memory:", check_same_thread=False)
        self._setup_tables(conn)
        recent_started = datetime.now(UTC) - timedelta(days=5)
        recent_ended = recent_started + timedelta(hours=2)
        old_started = datetime.now(UTC) - timedelta(days=120)
        old_ended = old_started + timedelta(hours=2)
        conn.executemany(
            """
            INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at)
            VALUES (?, ?, ?, ?)
            """,
            [
                (1, "target", recent_started.isoformat(), recent_ended.isoformat()),
                (2, "target", old_started.isoformat(), old_ended.isoformat()),
            ],
        )
        conn.executemany(
            """
            INSERT INTO twitch_session_chatters (
                session_id, streamer_login, chatter_login, messages
            ) VALUES (?, ?, ?, ?)
            """,
            [
                (1, "target", "window_user", 3),
                (2, "target", "window_user", 5),
            ],
        )
        conn.execute(
            """
            INSERT INTO twitch_chatter_rollup (
                streamer_login, chatter_login, total_sessions, total_messages, first_seen_at, last_seen_at
            ) VALUES (?, ?, ?, ?, ?, ?)
            """,
            (
                "target",
                "window_user",
                99,
                999,
                recent_started.isoformat(),
                recent_ended.isoformat(),
            ),
        )
        conn.commit()

        handler = _DummyViewers()
        request = SimpleNamespace(query={"streamer": "target", "days": "30"})
        try:
            with patch("bot.analytics.api_viewers.storage.readonly_connection", return_value=_ConnContext(conn)):
                response = await handler._api_v2_viewer_directory(request)
        finally:
            conn.close()

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertEqual(payload["days"], 30)
        self.assertEqual(payload["viewers"][0]["login"], "window_user")
        self.assertEqual(payload["viewers"][0]["totalSessions"], 1)
        self.assertEqual(payload["viewers"][0]["totalMessages"], 3)

    async def test_viewer_segments_respects_selected_window(self) -> None:
        conn = sqlite3.connect(":memory:", check_same_thread=False)
        self._setup_tables(conn)
        now = datetime.now(UTC)
        recent_started = now - timedelta(days=5)
        recent_ended = recent_started + timedelta(hours=2)
        old_started = now - timedelta(days=120)
        old_ended = old_started + timedelta(hours=2)
        conn.executemany(
            """
            INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at)
            VALUES (?, ?, ?, ?)
            """,
            [
                (1, "target", recent_started.isoformat(), recent_ended.isoformat()),
                (2, "target", old_started.isoformat(), old_ended.isoformat()),
            ],
        )
        conn.executemany(
            """
            INSERT INTO twitch_session_chatters (
                session_id, streamer_login, chatter_login, messages
            ) VALUES (?, ?, ?, ?)
            """,
            [
                (1, "target", "recent_user", 1),
                (2, "target", "old_user", 4),
            ],
        )
        conn.executemany(
            """
            INSERT INTO twitch_chatter_rollup (
                streamer_login, chatter_login, total_sessions, total_messages, first_seen_at, last_seen_at
            ) VALUES (?, ?, ?, ?, ?, ?)
            """,
            [
                ("target", "recent_user", 5, 10, (recent_started - timedelta(days=60)).isoformat(), recent_ended.isoformat()),
                ("target", "old_user", 7, 20, (old_started - timedelta(days=30)).isoformat(), old_ended.isoformat()),
            ],
        )
        conn.commit()

        handler = _DummyViewers()
        request = SimpleNamespace(query={"streamer": "target", "days": "30"})
        try:
            with patch("bot.analytics.api_viewers.storage.readonly_connection", return_value=_ConnContext(conn)):
                response = await handler._api_v2_viewer_segments(request)
        finally:
            conn.close()

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertEqual(payload["days"], 30)
        total_segment_viewers = sum(item["count"] for item in payload["segments"].values())
        self.assertEqual(total_segment_viewers, 1)

    async def test_viewer_tab_excludes_streamer_and_runtime_bot_identities(self) -> None:
        conn = sqlite3.connect(":memory:", check_same_thread=False)
        self._setup_tables(conn)
        recent_started = datetime.now(UTC) - timedelta(days=5)
        recent_ended = recent_started + timedelta(hours=2)
        conn.execute(
            """
            INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at)
            VALUES (?, ?, ?, ?)
            """,
            (1, "target", recent_started.isoformat(), recent_ended.isoformat()),
        )
        conn.executemany(
            """
            INSERT INTO twitch_session_chatters (
                session_id, streamer_login, chatter_login, messages
            ) VALUES (?, ?, ?, ?)
            """,
            [
                (1, "target", "target", 2),
                (1, "target", "deadlockbot", 4),
                (1, "target", "deutschedeadlockcommunity", 1),
                (1, "target", "real_viewer", 3),
                (1, "deutschedeadlockcommunity", "real_viewer", 2),
            ],
        )
        conn.executemany(
            """
            INSERT INTO twitch_chatter_rollup (
                streamer_login, chatter_login, total_sessions, total_messages, first_seen_at, last_seen_at
            ) VALUES (?, ?, ?, ?, ?, ?)
            """,
            [
                ("target", "target", 1, 2, recent_started.isoformat(), recent_ended.isoformat()),
                ("target", "deadlockbot", 1, 4, recent_started.isoformat(), recent_ended.isoformat()),
                (
                    "target",
                    "deutschedeadlockcommunity",
                    1,
                    1,
                    recent_started.isoformat(),
                    recent_ended.isoformat(),
                ),
                ("target", "real_viewer", 1, 3, recent_started.isoformat(), recent_ended.isoformat()),
                (
                    "deutschedeadlockcommunity",
                    "real_viewer",
                    1,
                    2,
                    recent_started.isoformat(),
                    recent_ended.isoformat(),
                ),
            ],
        )
        conn.commit()

        handler = _DummyViewers()
        handler._bot_token_manager = SimpleNamespace(bot_login="deutschedeadlockcommunity")
        handler._twitch_chat_bot = SimpleNamespace(
            nick="deadlockbot",
            _token_manager=SimpleNamespace(bot_login="deadlockbot"),
        )

        directory_request = SimpleNamespace(query={"streamer": "target", "days": "30"})
        segments_request = SimpleNamespace(query={"streamer": "target", "days": "30"})
        try:
            with patch("bot.analytics.api_viewers.storage.readonly_connection", return_value=_ConnContext(conn)):
                directory_response = await handler._api_v2_viewer_directory(directory_request)
                segments_response = await handler._api_v2_viewer_segments(segments_request)
        finally:
            conn.close()

        directory_payload = json.loads(directory_response.body.decode("utf-8"))
        segments_payload = json.loads(segments_response.body.decode("utf-8"))

        self.assertEqual(directory_response.status, 200)
        self.assertEqual([viewer["login"] for viewer in directory_payload["viewers"]], ["real_viewer"])
        self.assertEqual(directory_payload["summary"]["totalViewers"], 1)

        self.assertEqual(segments_response.status, 200)
        self.assertEqual(
            sum(item["count"] for item in segments_payload["segments"].values()),
            1,
        )
        self.assertEqual(
            segments_payload["crossChannelStats"]["topSharedChannels"],
            [],
        )

    async def test_viewer_segments_shared_channel_direction_is_not_hardcoded(self) -> None:
        conn = sqlite3.connect(":memory:", check_same_thread=False)
        self._setup_tables(conn)
        recent_started = datetime.now(UTC) - timedelta(days=5)
        recent_ended = recent_started + timedelta(hours=2)
        conn.executemany(
            """
            INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at)
            VALUES (?, ?, ?, ?)
            """,
            [
                (1, "target", recent_started.isoformat(), recent_ended.isoformat()),
                (2, "other_in", recent_started.isoformat(), recent_ended.isoformat()),
                (3, "other_out", recent_started.isoformat(), recent_ended.isoformat()),
            ],
        )
        conn.executemany(
            """
            INSERT INTO twitch_session_chatters (
                session_id, streamer_login, chatter_login, messages
            ) VALUES (?, ?, ?, ?)
            """,
            [
                (1, "target", "viewer_a", 2),
                (1, "target", "viewer_b", 3),
                (1, "target", "viewer_c", 1),
                (2, "other_in", "viewer_a", 4),
                (2, "other_in", "viewer_b", 5),
                (3, "other_out", "viewer_b", 2),
                (3, "other_out", "viewer_c", 2),
            ],
        )
        conn.executemany(
            """
            INSERT INTO twitch_chatter_rollup (
                streamer_login, chatter_login, total_sessions, total_messages, first_seen_at, last_seen_at
            ) VALUES (?, ?, ?, ?, ?, ?)
            """,
            [
                ("target", "viewer_a", 2, 2, (recent_started - timedelta(days=15)).isoformat(), recent_ended.isoformat()),
                ("target", "viewer_b", 3, 3, (recent_started - timedelta(days=15)).isoformat(), recent_ended.isoformat()),
                ("target", "viewer_c", 1, 1, (recent_started - timedelta(days=15)).isoformat(), recent_ended.isoformat()),
                ("other_in", "viewer_a", 4, 4, (recent_started - timedelta(days=24)).isoformat(), recent_ended.isoformat()),
                ("other_in", "viewer_b", 5, 5, (recent_started - timedelta(days=23)).isoformat(), recent_ended.isoformat()),
                ("other_out", "viewer_b", 2, 2, (recent_started - timedelta(days=5)).isoformat(), recent_ended.isoformat()),
                ("other_out", "viewer_c", 2, 2, (recent_started - timedelta(days=4)).isoformat(), recent_ended.isoformat()),
            ],
        )
        conn.commit()

        handler = _DummyViewers()
        request = SimpleNamespace(query={"streamer": "target", "days": "30"})
        try:
            with patch(
                "bot.analytics.api_viewers.storage.readonly_connection",
                return_value=_ConnContext(conn),
            ):
                response = await handler._api_v2_viewer_segments(request)
        finally:
            conn.close()

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        direction_map = {
            entry["streamer"]: entry["direction"]
            for entry in payload["crossChannelStats"]["topSharedChannels"]
        }
        self.assertEqual(direction_map["other_in"], "incoming")
        self.assertEqual(direction_map["other_out"], "outgoing")

    async def test_viewer_segments_uses_unknown_when_direction_cannot_be_inferred(self) -> None:
        conn = sqlite3.connect(":memory:", check_same_thread=False)
        self._setup_tables(conn)
        recent_started = datetime.now(UTC) - timedelta(days=5)
        recent_ended = recent_started + timedelta(hours=2)
        conn.executemany(
            """
            INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at)
            VALUES (?, ?, ?, ?)
            """,
            [
                (1, "target", recent_started.isoformat(), recent_ended.isoformat()),
                (2, "other_unknown", recent_started.isoformat(), recent_ended.isoformat()),
            ],
        )
        conn.executemany(
            """
            INSERT INTO twitch_session_chatters (
                session_id, streamer_login, chatter_login, messages
            ) VALUES (?, ?, ?, ?)
            """,
            [
                (1, "target", "viewer_a", 2),
                (2, "other_unknown", "viewer_a", 3),
            ],
        )
        conn.commit()

        handler = _DummyViewers()
        request = SimpleNamespace(query={"streamer": "target", "days": "30"})
        try:
            with patch(
                "bot.analytics.api_viewers.storage.readonly_connection",
                return_value=_ConnContext(conn),
            ):
                response = await handler._api_v2_viewer_segments(request)
        finally:
            conn.close()

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        direction_map = {
            entry["streamer"]: entry["direction"]
            for entry in payload["crossChannelStats"]["topSharedChannels"]
        }
        self.assertEqual(direction_map["other_unknown"], "unknown")


class CoachingSqlRegressionTests(unittest.TestCase):
    def test_schedule_optimizer_uses_valid_postgres_extract_casts(self) -> None:
        test_case = self

        class _GuardConn:
            def __init__(self) -> None:
                self.calls = 0

            def execute(self, sql, params=None):
                sql_text = str(sql)
                self.calls += 1
                self_ref = self

                class _Cursor:
                    def fetchall(self_inner):
                        if self_ref.calls == 1:
                            test_case.assertIn(
                                "EXTRACT(DOW FROM (ts_utc AT TIME ZONE 'UTC'))::int as weekday",
                                sql_text,
                            )
                            test_case.assertIn(
                                "EXTRACT(HOUR FROM (ts_utc AT TIME ZONE 'UTC'))::int as hour",
                                sql_text,
                            )
                        if self_ref.calls == 2:
                            test_case.assertIn(
                                "EXTRACT(DOW FROM (started_at AT TIME ZONE 'UTC'))::int as weekday",
                                sql_text,
                            )
                            test_case.assertIn(
                                "EXTRACT(HOUR FROM (started_at AT TIME ZONE 'UTC'))::int as hour",
                                sql_text,
                            )
                        return []

                return _Cursor()

        result = _schedule_optimizer(_GuardConn(), "target", "2026-02-01T00:00:00+00:00")
        self.assertEqual(result["competitionHeatmap"], [])
        self.assertEqual(result["yourCurrentSlots"], [])


class InternalHomeHealthScoreRegressionTests(unittest.TestCase):
    def test_health_score_uses_real_community_score_and_null_trend_without_previous_week(self) -> None:
        conn = sqlite3.connect(":memory:")
        conn.execute(
            """
            CREATE TABLE twitch_stats_tracked (
                streamer TEXT,
                ts_utc TEXT,
                viewer_count INTEGER
            )
            """
        )
        conn.execute(
            """
            CREATE TABLE twitch_stream_sessions (
                id INTEGER PRIMARY KEY,
                streamer_login TEXT,
                started_at TEXT,
                ended_at TEXT
            )
            """
        )
        conn.execute(
            """
            CREATE TABLE twitch_session_chatters (
                session_id INTEGER,
                chatter_login TEXT,
                chatter_id TEXT,
                is_first_time_streamer INTEGER
            )
            """
        )
        conn.execute(
            """
            CREATE TABLE twitch_chat_messages (
                streamer_login TEXT,
                message_ts TEXT
            )
            """
        )

        now = datetime.now(UTC)
        current_ts = (now - timedelta(days=2)).isoformat()
        conn.executemany(
            "INSERT INTO twitch_stats_tracked (streamer, ts_utc, viewer_count) VALUES (?, ?, ?)",
            [
                ("target", current_ts, 20),
                ("target", (now - timedelta(days=1)).isoformat(), 30),
            ],
        )
        conn.execute(
            """
            INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at)
            VALUES (?, ?, ?, ?)
            """,
            (1, "target", current_ts, (now - timedelta(days=2, hours=-2)).isoformat()),
        )
        conn.executemany(
            """
            INSERT INTO twitch_session_chatters (session_id, chatter_login, chatter_id, is_first_time_streamer)
            VALUES (?, ?, ?, ?)
            """,
            [
                (1, "target", None, 0),
                (1, "nightbot", None, 0),
                (1, "viewer_a", None, 0),
                (1, "viewer_b", None, 1),
            ],
        )
        conn.executemany(
            "INSERT INTO twitch_chat_messages (streamer_login, message_ts) VALUES (?, ?)",
            [("target", current_ts), ("target", current_ts)],
        )
        conn.commit()

        try:
            payload = _compute_health_score("target", _CompatSqliteConn(conn))
        finally:
            conn.close()

        self.assertIsNotNone(payload)
        assert payload is not None
        self.assertIsNone(payload["trend"])
        self.assertEqual(payload["sub_scores"]["community"], 50)


class ChatHypeTimelineRegressionTests(unittest.IsolatedAsyncioTestCase):
    async def test_chat_hype_timeline_skips_null_viewer_minutes(self) -> None:
        handler = _DummyV2()
        request = SimpleNamespace(query={"streamer": "target", "session_id": "1"})

        with (
            patch(
                "bot.analytics.api_chat_deep.storage.readonly_connection",
                return_value=_ConnContext(_ChatHypeTimelineConn()),
            ),
            patch(
                "bot.analytics.api_chat_deep.build_raw_chat_status",
                return_value={"available": True, "suspectedIngestionIssue": False},
            ),
        ):
            response = await handler._api_v2_chat_hype_timeline(request)

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertEqual(len(payload["timeline"]), 2)
        self.assertEqual(payload["timeline"][0]["minute"], 0)
        self.assertEqual(payload["timeline"][0]["viewers"], 42)
        self.assertEqual(payload["timeline"][1]["minute"], 1)
        self.assertEqual(payload["timeline"][1]["viewers"], 43)


class SessionDetailRegressionTests(unittest.IsolatedAsyncioTestCase):
    @staticmethod
    def _setup_tables(conn: sqlite3.Connection) -> None:
        conn.execute(
            """
            CREATE TABLE twitch_stream_sessions (
                id INTEGER PRIMARY KEY,
                streamer_login TEXT,
                started_at TEXT,
                ended_at TEXT,
                duration_seconds INTEGER,
                start_viewers INTEGER,
                peak_viewers INTEGER,
                end_viewers INTEGER,
                avg_viewers REAL,
                retention_5m REAL,
                retention_10m REAL,
                retention_20m REAL,
                dropoff_pct REAL,
                unique_chatters INTEGER,
                first_time_chatters INTEGER,
                returning_chatters INTEGER,
                stream_title TEXT
            )
            """
        )
        conn.execute(
            """
            CREATE TABLE twitch_session_chatters (
                session_id INTEGER,
                chatter_login TEXT,
                chatter_id TEXT,
                messages INTEGER,
                is_first_time_streamer INTEGER
            )
            """
        )
        conn.execute(
            """
            CREATE TABLE twitch_session_viewers (
                session_id INTEGER,
                minutes_from_start INTEGER,
                viewer_count INTEGER
            )
            """
        )

    async def test_session_detail_falls_back_to_legacy_counts_without_chatter_rows(self) -> None:
        conn = sqlite3.connect(":memory:", check_same_thread=False)
        self._setup_tables(conn)
        conn.execute(
            """
            INSERT INTO twitch_stream_sessions (
                id, streamer_login, started_at, ended_at, duration_seconds,
                start_viewers, peak_viewers, end_viewers, avg_viewers,
                retention_5m, retention_10m, retention_20m, dropoff_pct,
                unique_chatters, first_time_chatters, returning_chatters, stream_title
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                1,
                "target",
                "2026-02-01T12:00:00+00:00",
                "2026-02-01T14:00:00+00:00",
                7200,
                20,
                35,
                25,
                24.5,
                0.8,
                0.7,
                0.6,
                0.2,
                12,
                7,
                5,
                "Legacy Session",
            ),
        )
        conn.commit()

        handler = _DummyV2()
        request = SimpleNamespace(match_info={"id": "1"})
        try:
            with patch("bot.storage.pg.readonly_connection", return_value=_ConnContext(conn)):
                response = await handler._api_v2_session_detail(request)
        finally:
            conn.close()

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertEqual(payload["uniqueChatters"], 12)
        self.assertEqual(payload["firstTimeChatters"], 7)
        self.assertEqual(payload["returningChatters"], 5)

    async def test_session_detail_bot_only_rows_return_zero_not_legacy(self) -> None:
        conn = sqlite3.connect(":memory:", check_same_thread=False)
        self._setup_tables(conn)
        conn.execute(
            """
            INSERT INTO twitch_stream_sessions (
                id, streamer_login, started_at, ended_at, duration_seconds,
                start_viewers, peak_viewers, end_viewers, avg_viewers,
                retention_5m, retention_10m, retention_20m, dropoff_pct,
                unique_chatters, first_time_chatters, returning_chatters, stream_title
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                1,
                "target",
                "2026-02-01T12:00:00+00:00",
                "2026-02-01T14:00:00+00:00",
                7200,
                20,
                35,
                25,
                24.5,
                0.8,
                0.7,
                0.6,
                0.2,
                12,
                7,
                5,
                "Bot-only Session",
            ),
        )
        conn.execute(
            """
            INSERT INTO twitch_session_chatters (
                session_id, chatter_login, chatter_id, messages, is_first_time_streamer
            ) VALUES (?, ?, ?, ?, ?)
            """,
            (1, "nightbot", "bot_1", 15, 0),
        )
        conn.commit()

        handler = _DummyV2()
        request = SimpleNamespace(match_info={"id": "1"})
        try:
            with patch("bot.storage.pg.readonly_connection", return_value=_ConnContext(conn)):
                response = await handler._api_v2_session_detail(request)
        finally:
            conn.close()

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertEqual(payload["uniqueChatters"], 0)
        self.assertEqual(payload["firstTimeChatters"], 0)
        self.assertEqual(payload["returningChatters"], 0)


class OverviewSessionsRegressionTests(unittest.TestCase):
    @staticmethod
    def _setup_overview_tables(conn: sqlite3.Connection) -> None:
        conn.execute(
            """
            CREATE TABLE twitch_stream_sessions (
                id INTEGER PRIMARY KEY,
                streamer_login TEXT,
                started_at TEXT,
                ended_at TEXT,
                duration_seconds INTEGER,
                start_viewers INTEGER,
                peak_viewers INTEGER,
                end_viewers INTEGER,
                avg_viewers REAL,
                retention_5m REAL,
                retention_10m REAL,
                retention_20m REAL,
                dropoff_pct REAL,
                unique_chatters INTEGER,
                first_time_chatters INTEGER,
                returning_chatters INTEGER,
                followers_start INTEGER,
                followers_end INTEGER,
                stream_title TEXT,
                follower_delta INTEGER
            )
            """
        )
        conn.execute(
            """
            CREATE TABLE twitch_session_chatters (
                session_id INTEGER,
                chatter_login TEXT,
                chatter_id TEXT,
                messages INTEGER,
                is_first_time_streamer INTEGER,
                seen_via_chatters_api INTEGER
            )
            """
        )

    def test_get_sessions_bot_only_rows_do_not_fallback_to_legacy_counts(self) -> None:
        conn = sqlite3.connect(":memory:")
        self._setup_overview_tables(conn)
        conn.execute(
            """
            INSERT INTO twitch_stream_sessions (
                id, streamer_login, started_at, ended_at, duration_seconds,
                start_viewers, peak_viewers, end_viewers, avg_viewers,
                retention_5m, retention_10m, retention_20m, dropoff_pct,
                unique_chatters, first_time_chatters, returning_chatters,
                followers_start, followers_end, stream_title
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                1,
                "target",
                "2026-02-01T12:00:00+00:00",
                "2026-02-01T14:00:00+00:00",
                7200,
                20,
                35,
                25,
                24.5,
                0.8,
                0.7,
                0.6,
                0.2,
                12,
                7,
                5,
                100,
                104,
                "Bot-only Session",
            ),
        )
        conn.execute(
            """
            INSERT INTO twitch_session_chatters (
                session_id, chatter_login, chatter_id, messages, is_first_time_streamer
            ) VALUES (?, ?, ?, ?, ?)
            """,
            (1, "nightbot", "bot_1", 15, 0),
        )
        conn.commit()

        handler = _DummyOverview()
        compat_conn = _CompatSqliteConn(conn)
        sessions = handler._get_sessions(
            conn=compat_conn,
            since_date="2026-01-01T00:00:00+00:00",
            streamer="target",
            limit=10,
        )
        conn.close()

        self.assertEqual(len(sessions), 1)
        self.assertEqual(sessions[0]["uniqueChatters"], 0)
        self.assertEqual(sessions[0]["firstTimeChatters"], 0)
        self.assertEqual(sessions[0]["returningChatters"], 0)
        self.assertEqual(sessions[0]["startViewers"], 20)
        self.assertEqual(sessions[0]["peakViewers"], 35)

    def test_get_sessions_without_chatter_rows_falls_back_to_legacy_counts(self) -> None:
        conn = sqlite3.connect(":memory:")
        self._setup_overview_tables(conn)
        conn.execute(
            """
            INSERT INTO twitch_stream_sessions (
                id, streamer_login, started_at, ended_at, duration_seconds,
                start_viewers, peak_viewers, end_viewers, avg_viewers,
                retention_5m, retention_10m, retention_20m, dropoff_pct,
                unique_chatters, first_time_chatters, returning_chatters,
                followers_start, followers_end, stream_title
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                1,
                "target",
                "2026-02-01T12:00:00+00:00",
                "2026-02-01T14:00:00+00:00",
                7200,
                20,
                35,
                25,
                24.5,
                0.8,
                0.7,
                0.6,
                0.2,
                12,
                7,
                5,
                100,
                104,
                "No Chatter Rows",
            ),
        )
        conn.commit()

        handler = _DummyOverview()
        compat_conn = _CompatSqliteConn(conn)
        sessions = handler._get_sessions(
            conn=compat_conn,
            since_date="2026-01-01T00:00:00+00:00",
            streamer="target",
            limit=10,
        )
        conn.close()

        self.assertEqual(len(sessions), 1)
        self.assertEqual(sessions[0]["uniqueChatters"], 12)
        self.assertEqual(sessions[0]["firstTimeChatters"], 7)
        self.assertEqual(sessions[0]["returningChatters"], 5)

    def test_calculate_overview_metrics_falls_back_to_legacy_counts_per_session(self) -> None:
        conn = sqlite3.connect(":memory:")
        conn.create_function(
            "LEAST",
            -1,
            lambda *vals: min((v for v in vals if v is not None), default=None),
        )
        self._setup_overview_tables(conn)
        conn.executemany(
            """
            INSERT INTO twitch_stream_sessions (
                id, streamer_login, started_at, ended_at, duration_seconds,
                start_viewers, peak_viewers, end_viewers, avg_viewers,
                retention_5m, retention_10m, retention_20m, dropoff_pct,
                unique_chatters, first_time_chatters, returning_chatters,
                followers_start, followers_end, stream_title, follower_delta
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            [
                (
                    1,
                    "target",
                    "2026-02-01T12:00:00+00:00",
                    "2026-02-01T14:00:00+00:00",
                    7200,
                    20,
                    40,
                    25,
                    20.0,
                    0.8,
                    0.7,
                    0.6,
                    0.2,
                    12,
                    7,
                    5,
                    100,
                    104,
                    "Legacy-only session",
                    4,
                ),
                (
                    2,
                    "target",
                    "2026-02-02T12:00:00+00:00",
                    "2026-02-02T14:00:00+00:00",
                    7200,
                    30,
                    50,
                    35,
                    25.0,
                    0.85,
                    0.75,
                    0.65,
                    0.22,
                    99,
                    50,
                    49,
                    104,
                    107,
                    "Session with chatter rows",
                    3,
                ),
            ],
        )
        conn.executemany(
            """
            INSERT INTO twitch_session_chatters (
                session_id, chatter_login, chatter_id, messages, is_first_time_streamer, seen_via_chatters_api
            ) VALUES (?, ?, ?, ?, ?, ?)
            """,
            [
                (2, "viewer_a", "a", 3, 1, 1),
                (2, "viewer_b", "b", 1, 0, 0),
            ],
        )
        conn.commit()

        handler = _DummyOverview()
        compat_conn = _CompatSqliteConn(conn)
        metrics = handler._calculate_overview_metrics(
            conn=compat_conn,
            since_date="2026-01-01T00:00:00+00:00",
            streamer=None,
        )
        conn.close()

        self.assertEqual(metrics["total_unique_chatters"], 14)
        self.assertAlmostEqual(metrics["chat_per_100"], 17.0, places=3)
        self.assertEqual(metrics["active_chatters"], 2)

    def test_get_sessions_uses_psycopg_placeholders_for_bot_filter(self) -> None:
        handler = _DummyOverview()
        sessions = handler._get_sessions(
            conn=_OverviewPlaceholderParityConn(),
            since_date="2026-01-01T00:00:00+00:00",
            streamer="target",
            limit=10,
        )

        self.assertEqual(sessions, [])

    def test_calculate_overview_metrics_uses_psycopg_placeholders_for_bot_filters(self) -> None:
        handler = _DummyOverview()
        metrics = handler._calculate_overview_metrics(
            conn=_OverviewPlaceholderParityConn(),
            since_date="2026-01-01T00:00:00+00:00",
            streamer="target",
        )

        self.assertEqual(metrics["total_unique_chatters"], 0)
        self.assertEqual(metrics["active_chatters"], 0)


class BotClausePlaceholderRegressionTests(unittest.TestCase):
    def test_runtime_queries_use_psycopg_placeholders_for_known_bot_filters(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        runtime_files = [
            Path("bot/analytics/api_audience.py"),
            Path("bot/analytics/api_chat_deep.py"),
            Path("bot/analytics/api_insights.py"),
            Path("bot/analytics/api_overview.py"),
            Path("bot/analytics/api_raids.py"),
            Path("bot/analytics/api_v2.py"),
            Path("bot/analytics/api_viewers.py"),
            Path("bot/analytics/mixin.py"),
            Path("bot/analytics/raid_metrics.py"),
            Path("bot/chat/promos.py"),
        ]

        missing_placeholder: list[str] = []
        wrong_placeholder: list[str] = []

        for rel_path in runtime_files:
            source = (repo_root / rel_path).read_text(encoding="utf-8")
            tree = ast.parse(source, filename=str(rel_path))
            for node in ast.walk(tree):
                if not isinstance(node, ast.Call):
                    continue
                if not isinstance(node.func, ast.Name):
                    continue
                if node.func.id != "build_known_chat_bot_not_in_clause":
                    continue

                placeholder_kw = next(
                    (kw for kw in node.keywords if kw.arg == "placeholder"),
                    None,
                )
                location = f"{rel_path}:{node.lineno}"
                if placeholder_kw is None:
                    missing_placeholder.append(location)
                    continue
                if not (
                    isinstance(placeholder_kw.value, ast.Constant)
                    and placeholder_kw.value.value == "%s"
                ):
                    wrong_placeholder.append(location)

        self.assertEqual(
            missing_placeholder,
            [],
            msg="Missing placeholder=\"%s\" for known chat bot filters: "
            + ", ".join(missing_placeholder),
        )
        self.assertEqual(
            wrong_placeholder,
            [],
            msg="Unexpected placeholder for known chat bot filters: "
            + ", ".join(wrong_placeholder),
        )


class OverviewRaidRetentionRegressionTests(unittest.IsolatedAsyncioTestCase):
    async def test_raid_retention_zero_viewer_rows_are_not_dropped(self) -> None:
        rows = [
            (
                1,
                "source_channel",
                "target_channel",
                0,
                "2026-02-01T12:00:00+00:00",
                999,
            )
        ]

        def _fake_metrics(_conn, raids):
            return {
                raid_identity_key(raid["raid_id"], raid["executed_at"]): {
                    "plus5m": 0,
                    "plus15m": 0,
                    "plus30m": 0,
                    "known_from_raider": 0,
                    "new_chatters": 0,
                }
                for raid in raids
            }

        handler = _DummyOverview()
        request = SimpleNamespace(query={"streamer": "source_channel", "days": "90"})
        with (
            patch(
                "bot.analytics.api_overview.storage.readonly_connection",
                return_value=_ConnContext(_OverviewRaidRetentionConn(rows)),
            ),
            patch(
                "bot.analytics.api_overview.recalculate_raid_chat_metrics",
                side_effect=_fake_metrics,
            ),
        ):
            response = await handler._api_v2_raid_retention(request)

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertTrue(payload["dataAvailable"])
        self.assertEqual(payload["summary"]["raidCount"], 1)
        self.assertEqual(len(payload["raids"]), 1)
        self.assertEqual(payload["raids"][0]["viewersSent"], 0)
        self.assertEqual(payload["raids"][0]["retention30mPct"], 0.0)
        self.assertEqual(payload["raids"][0]["chatterConversionPct"], 0.0)
        self.assertTrue(payload["dataQuality"]["botFilterApplied"])
        self.assertEqual(payload["dataQuality"]["raidMetricSource"], "recalculated")
        self.assertEqual(payload["dataQuality"]["recalculatedRaidCount"], 1)
        self.assertEqual(payload["dataQuality"]["storedFallbackRaidCount"], 0)

    async def test_raid_retention_keeps_duplicate_raid_ids_split_by_executed_at(self) -> None:
        rows = [
            (
                9,
                "source_channel",
                "target_channel",
                10,
                "2026-02-02T12:00:00+00:00",
                1001,
            ),
            (
                9,
                "source_channel",
                "target_channel",
                10,
                "2026-02-01T12:00:00+00:00",
                1000,
            ),
        ]

        def _fake_metrics(_conn, _raids):
            return {
                raid_identity_key(9, "2026-02-02T12:00:00+00:00"): {
                    "plus5m": 1,
                    "plus15m": 3,
                    "plus30m": 7,
                    "known_from_raider": 2,
                    "new_chatters": 4,
                },
                raid_identity_key(9, "2026-02-01T12:00:00+00:00"): {
                    "plus5m": 2,
                    "plus15m": 4,
                    "plus30m": 5,
                    "known_from_raider": 1,
                    "new_chatters": 1,
                },
            }

        handler = _DummyOverview()
        request = SimpleNamespace(query={"streamer": "source_channel", "days": "90"})
        with (
            patch(
                "bot.analytics.api_overview.storage.readonly_connection",
                return_value=_ConnContext(_OverviewRaidRetentionConn(rows)),
            ),
            patch(
                "bot.analytics.api_overview.recalculate_raid_chat_metrics",
                side_effect=_fake_metrics,
            ),
        ):
            response = await handler._api_v2_raid_retention(request)

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertEqual(len(payload["raids"]), 2)
        self.assertEqual(payload["raids"][0]["raidId"], 9)
        self.assertEqual(payload["raids"][0]["executedAt"], "2026-02-02T12:00:00+00:00")
        self.assertEqual(payload["raids"][0]["chattersAt30m"], 7)
        self.assertEqual(payload["raids"][1]["raidId"], 9)
        self.assertEqual(payload["raids"][1]["executedAt"], "2026-02-01T12:00:00+00:00")
        self.assertEqual(payload["raids"][1]["chattersAt30m"], 5)

    async def test_raid_retention_uses_stored_metrics_when_target_session_missing(self) -> None:
        rows = [
            (
                2,
                "source_channel",
                "target_channel",
                10,
                "2026-02-01T12:00:00+00:00",
                None,
                4,
                5,
                6,
                2,
                3,
            )
        ]

        handler = _DummyOverview()
        request = SimpleNamespace(query={"streamer": "source_channel", "days": "90"})
        with (
            patch(
                "bot.analytics.api_overview.storage.readonly_connection",
                return_value=_ConnContext(_OverviewRaidRetentionConn(rows)),
            ),
            patch(
                "bot.analytics.api_overview.recalculate_raid_chat_metrics",
                return_value={},
            ) as metrics_mock,
        ):
            response = await handler._api_v2_raid_retention(request)

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertTrue(payload["dataAvailable"])
        self.assertEqual(metrics_mock.call_count, 1)
        self.assertEqual(len(payload["raids"]), 1)
        self.assertEqual(payload["summary"]["raidCount"], 1)
        self.assertEqual(payload["raids"][0]["chattersAt5m"], 4)
        self.assertEqual(payload["raids"][0]["chattersAt15m"], 5)
        self.assertEqual(payload["raids"][0]["chattersAt30m"], 6)
        self.assertEqual(payload["raids"][0]["newChatters"], 2)
        self.assertEqual(payload["raids"][0]["knownFromRaider"], 3)
        self.assertEqual(payload["raids"][0]["retention30mPct"], 60.0)
        self.assertEqual(payload["raids"][0]["chatterConversionPct"], 20.0)
        self.assertFalse(payload["dataQuality"]["botFilterApplied"])
        self.assertEqual(payload["dataQuality"]["raidMetricSource"], "stored")
        self.assertEqual(payload["dataQuality"]["recalculatedRaidCount"], 0)
        self.assertEqual(payload["dataQuality"]["storedFallbackRaidCount"], 1)


@unittest.skipUnless(
    os.environ.get("TWITCH_ANALYTICS_DSN"),
    "requires TWITCH_ANALYTICS_DSN for PostgreSQL SQL execution regression test",
)
class RaidMetricsSqlRegressionTests(unittest.TestCase):
    def test_recalculate_raid_chat_metrics_executes_postgres_sql(self) -> None:
        from bot.analytics.raid_metrics import recalculate_raid_chat_metrics
        from bot.storage import pg as storage_pg

        with storage_pg.transaction() as conn:
            conn.execute(
                """
                CREATE TEMP TABLE twitch_session_chatters (
                    session_id BIGINT NOT NULL,
                    chatter_login TEXT,
                    chatter_id TEXT,
                    first_message_at TIMESTAMPTZ NOT NULL,
                    messages INTEGER NOT NULL DEFAULT 0,
                    last_seen_at TIMESTAMPTZ
                )
                """
            )
            conn.execute(
                """
                CREATE TEMP TABLE twitch_chatter_rollup (
                    streamer_login TEXT NOT NULL,
                    chatter_login TEXT NOT NULL,
                    first_seen_at TIMESTAMPTZ NOT NULL
                )
                """
            )
            conn.executemany(
                """
                INSERT INTO twitch_session_chatters (
                    session_id, chatter_login, chatter_id, first_message_at, messages, last_seen_at
                ) VALUES (%s, %s, %s, %s, %s, %s)
                """,
                [
                    (42, "viewer_a", "id_a", "2026-02-01T12:01:00+00:00", 3, "2026-02-01T12:03:00+00:00"),
                    (42, "viewer_b", "id_b", "2026-02-01T12:02:00+00:00", 2, "2026-02-01T12:12:00+00:00"),
                    (42, "viewer_c", "id_c", "2026-02-01T12:10:00+00:00", 1, "2026-02-01T12:25:00+00:00"),
                    (42, "nightbot", "id_bot", "2026-02-01T12:01:30+00:00", 10, "2026-02-01T12:02:30+00:00"),
                    (42, None, "anon_1", "2026-02-01T12:05:00+00:00", 1, "2026-02-01T12:06:00+00:00"),
                    (42, "viewer_d", "id_d", "2026-02-01T11:59:00+00:00", 4, "2026-02-01T12:40:00+00:00"),
                ],
            )
            conn.executemany(
                """
                INSERT INTO twitch_chatter_rollup (streamer_login, chatter_login, first_seen_at)
                VALUES (%s, %s, %s)
                """,
                [
                    ("raider_x", "viewer_a", "2026-01-01T00:00:00+00:00"),
                    ("raider_x", "viewer_b", "2026-01-01T00:00:00+00:00"),
                    ("target_y", "viewer_b", "2026-01-01T00:00:00+00:00"),
                ],
            )

            metrics = recalculate_raid_chat_metrics(
                conn,
                [
                    {
                        "raid_id": 9001,
                        "target_session_id": 42,
                        "executed_at": "2026-02-01T12:00:00+00:00",
                        "from_login": "raider_x",
                        "to_login": "target_y",
                    }
                ],
            )

        raid_key = raid_identity_key(9001, "2026-02-01T12:00:00+00:00")
        assert raid_key is not None
        self.assertIn(raid_key, metrics)
        self.assertEqual(metrics[raid_key]["plus5m"], 1)
        self.assertEqual(metrics[raid_key]["plus15m"], 3)
        self.assertEqual(metrics[raid_key]["plus30m"], 4)
        self.assertEqual(metrics[raid_key]["known_from_raider"], 2)
        self.assertEqual(metrics[raid_key]["new_chatters"], 3)

    def test_recalculate_raid_chat_metrics_keeps_duplicate_raid_ids_split_by_executed_at(self) -> None:
        from bot.analytics.raid_metrics import recalculate_raid_chat_metrics
        from bot.storage import pg as storage_pg

        with storage_pg.transaction() as conn:
            conn.execute(
                """
                CREATE TEMP TABLE twitch_session_chatters (
                    session_id BIGINT NOT NULL,
                    chatter_login TEXT,
                    chatter_id TEXT,
                    first_message_at TIMESTAMPTZ NOT NULL,
                    messages INTEGER NOT NULL DEFAULT 0,
                    last_seen_at TIMESTAMPTZ
                )
                """
            )
            conn.execute(
                """
                CREATE TEMP TABLE twitch_chatter_rollup (
                    streamer_login TEXT NOT NULL,
                    chatter_login TEXT NOT NULL,
                    first_seen_at TIMESTAMPTZ NOT NULL
                )
                """
            )
            conn.executemany(
                """
                INSERT INTO twitch_session_chatters (
                    session_id, chatter_login, chatter_id, first_message_at, messages, last_seen_at
                ) VALUES (%s, %s, %s, %s, %s, %s)
                """,
                [
                    (42, "viewer_a", "id_a", "2026-02-01T12:01:00+00:00", 1, "2026-02-01T12:04:00+00:00"),
                    (42, "viewer_b", "id_b", "2026-02-01T12:06:00+00:00", 1, "2026-02-01T12:20:00+00:00"),
                    (43, "viewer_c", "id_c", "2026-02-02T12:03:00+00:00", 1, "2026-02-02T12:06:00+00:00"),
                    (43, "viewer_d", "id_d", "2026-02-02T12:07:00+00:00", 1, "2026-02-02T12:18:00+00:00"),
                ],
            )
            metrics = recalculate_raid_chat_metrics(
                conn,
                [
                    {
                        "raid_id": 9002,
                        "target_session_id": 42,
                        "executed_at": "2026-02-01T12:00:00+00:00",
                        "from_login": "raider_x",
                        "to_login": "target_y",
                    },
                    {
                        "raid_id": 9002,
                        "target_session_id": 43,
                        "executed_at": "2026-02-02T12:00:00+00:00",
                        "from_login": "raider_x",
                        "to_login": "target_y",
                    },
                ],
            )

        older_key = raid_identity_key(9002, "2026-02-01T12:00:00+00:00")
        newer_key = raid_identity_key(9002, "2026-02-02T12:00:00+00:00")
        assert older_key is not None
        assert newer_key is not None
        self.assertIn(older_key, metrics)
        self.assertIn(newer_key, metrics)
        self.assertEqual(metrics[older_key]["plus30m"], 2)
        self.assertEqual(metrics[newer_key]["plus30m"], 2)
        self.assertEqual(metrics[older_key]["plus5m"], 1)
        self.assertEqual(metrics[newer_key]["plus5m"], 1)


class ChatSocialGraphMentionRegexRegressionTests(unittest.TestCase):
    def test_mentions_require_a_real_boundary_and_full_handle(self) -> None:
        valid_handle = "a" * 25
        too_long_handle = "b" * 26
        content = (
            f"mail foo@bar.com prefix@embedded @{valid_handle} "
            f"@{too_long_handle} and @ok_name"
        )

        matches = _MENTION_RE.findall(content)

        self.assertEqual(matches, [valid_handle, "ok_name"])


if __name__ == "__main__":
    unittest.main()
