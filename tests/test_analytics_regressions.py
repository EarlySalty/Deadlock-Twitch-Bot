import json
import os
import sqlite3
import unittest
from datetime import UTC, datetime
from types import SimpleNamespace
from unittest.mock import patch

from bot.analytics.api_audience import _AnalyticsAudienceMixin
from bot.analytics.api_insights import _AnalyticsInsightsMixin
from bot.analytics.api_overview import _AnalyticsOverviewMixin
from bot.analytics.api_raids import _AnalyticsRaidsMixin
from bot.analytics.raid_metrics import raid_identity_key
from bot.analytics.api_viewers import _AnalyticsViewersMixin
from bot.analytics.api_v2 import AnalyticsV2Mixin


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


class _OverviewRaidRetentionConn:
    def __init__(self, rows):
        self._rows = rows

    def execute(self, sql, params=None):
        if "FROM twitch_raid_retention" in sql:
            return _FakeCursor(self._rows)
        raise AssertionError(f"Unexpected SQL in overview raid retention test: {sql[:200]}")


class _ChatAnalyticsSqlGuardConn:
    def __init__(self):
        self.checked_first_time_cast = False

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
        self.assertTrue(payload["dataQuality"]["botFilterApplied"])


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
        conn = sqlite3.connect(":memory:")
        self._setup_tables(conn)
        conn.execute(
            """
            INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at)
            VALUES (?, ?, ?, ?)
            """,
            (
                1,
                "target",
                "2026-02-25T20:00:00+00:00",
                "2026-02-25T22:00:00+00:00",
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
                "2026-02-25T20:00:00+00:00",
                "2026-02-25T20:00:00+00:00",
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
                "2026-02-25T20:05:00+00:00",
                None,
                "2026-02-25T20:05:00+00:00",
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
        conn = sqlite3.connect(":memory:")
        self._setup_tables(conn)
        conn.executemany(
            """
            INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at)
            VALUES (?, ?, ?, ?)
            """,
            [
                (1, "target", "2026-02-25T20:00:00+00:00", "2026-02-25T22:00:00+00:00"),
                (2, "target", "2025-12-15T20:00:00+00:00", "2025-12-15T22:00:00+00:00"),
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
                "2026-02-25T20:00:00+00:00",
                "2026-02-25T22:00:00+00:00",
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
        conn = sqlite3.connect(":memory:")
        self._setup_tables(conn)
        conn.executemany(
            """
            INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at)
            VALUES (?, ?, ?, ?)
            """,
            [
                (1, "target", "2026-02-21T20:00:00+00:00", "2026-02-21T22:00:00+00:00"),
                (2, "target", "2025-12-10T20:00:00+00:00", "2025-12-10T22:00:00+00:00"),
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
                ("target", "recent_user", 5, 10, "2025-11-01T20:00:00+00:00", "2026-02-21T22:00:00+00:00"),
                ("target", "old_user", 7, 20, "2025-10-01T20:00:00+00:00", "2025-12-10T22:00:00+00:00"),
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
        conn = sqlite3.connect(":memory:")
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
        conn = sqlite3.connect(":memory:")
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


if __name__ == "__main__":
    unittest.main()
