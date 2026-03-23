import contextlib
import sqlite3
import time
import unittest
from datetime import UTC, datetime, timedelta
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from bot.monitoring.sessions_mixin import _SessionsMixin
from bot.raid.bot import RaidBot
from bot.raid.partner_raid_score_tracking import (
    resolve_partner_raid_tracking_for_session,
    track_confirmed_partner_raid,
)

from tests.sqlite_twitch_schema import ensure_sqlite_twitch_schema


class _CompatConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    @staticmethod
    def _translate(sql: str) -> str:
        translated = str(sql).replace("%s", "?")
        translated = translated.replace("NOW() - INTERVAL '1 day'", "datetime('now', '-1 day')")
        translated = translated.replace("NOW() - INTERVAL '7 days'", "datetime('now', '-7 days')")
        translated = translated.replace("NOW() - INTERVAL '30 days'", "datetime('now', '-30 days')")
        return translated

    def execute(self, sql: str, params=()):
        return self._conn.execute(self._translate(sql), params)

    def __getattr__(self, name: str):
        return getattr(self._conn, name)


def _iso_utc(value: datetime) -> str:
    return value.astimezone(UTC).isoformat(timespec="seconds")


class PartnerRaidScoreTrackingTests(unittest.TestCase):
    def _make_conn(self) -> sqlite3.Connection:
        conn = sqlite3.connect(":memory:")
        conn.row_factory = sqlite3.Row
        ensure_sqlite_twitch_schema(conn)
        return conn

    def test_track_confirmed_partner_raid_stores_score_snapshot(self) -> None:
        conn = self._make_conn()
        confirmed_at = datetime(2026, 3, 9, 20, 0, tzinfo=UTC)
        conn.execute(
            """
            INSERT INTO twitch_live_state (
                twitch_user_id, streamer_login, is_live, last_started_at, last_game, active_session_id
            ) VALUES (?, ?, 1, ?, ?, ?)
            """,
            ("2002", "target_login", _iso_utc(confirmed_at - timedelta(hours=1)), "Deadlock", 77),
        )
        conn.execute(
            """
            INSERT INTO twitch_raid_history (
                from_broadcaster_id, from_broadcaster_login,
                to_broadcaster_id, to_broadcaster_login,
                executed_at, success
            ) VALUES (?, ?, ?, ?, ?, 1)
            """,
            ("1001", "source_login", "2002", "target_login", _iso_utc(confirmed_at - timedelta(minutes=1))),
        )
        conn.execute(
            """
            INSERT INTO twitch_raid_history (
                from_broadcaster_id, from_broadcaster_login,
                to_broadcaster_id, to_broadcaster_login,
                executed_at, success
            ) VALUES (?, ?, ?, ?, ?, 1)
            """,
            ("wrong-source", "source_login", "2002", "target_login", _iso_utc(confirmed_at)),
        )
        conn.execute(
            """
            INSERT INTO twitch_raid_history (
                from_broadcaster_id, from_broadcaster_login,
                to_broadcaster_id, to_broadcaster_login,
                executed_at, success
            ) VALUES (?, ?, ?, ?, ?, ?)
            """,
            ("1001", "source_login", "2002", "target_login", _iso_utc(confirmed_at + timedelta(minutes=1)), False),
        )
        conn.commit()

        with patch(
            "bot.raid.partner_raid_score_tracking.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            tracking_id = track_confirmed_partner_raid(
                to_broadcaster_id="2002",
                to_broadcaster_login="target_login",
                from_broadcaster_login="source_login",
                from_broadcaster_id="1001",
                viewer_count=42,
                confirmed_at=confirmed_at,
                score_snapshot={
                    "final_score": 1.125,
                    "base_score": 0.625,
                    "duration_score": 0.5,
                    "time_pattern_score": 0.75,
                    "new_partner_multiplier": 1.2,
                    "raid_boost_multiplier": 1.5,
                    "today_received_raids": 1,
                    "last_computed_at": "2026-03-09T19:55:00+00:00",
                },
            )

        self.assertIsNotNone(tracking_id)
        row = conn.execute(
            "SELECT * FROM twitch_partner_raid_score_tracking WHERE id = ?",
            (tracking_id,),
        ).fetchone()
        self.assertIsNotNone(row)
        assert row is not None
        self.assertEqual(int(row["raid_history_id"]), 1)
        self.assertEqual(
            row["raid_history_executed_at"],
            _iso_utc(confirmed_at - timedelta(minutes=1)),
        )
        self.assertEqual(int(row["target_session_id"]), 77)
        self.assertEqual(int(row["was_deadlock_at_raid"]), 1)
        self.assertAlmostEqual(float(row["final_score"]), 1.125, places=6)
        self.assertIsNone(row["resolved_at"])
        conn.close()

    def test_track_confirmed_partner_raid_prefers_latest_executed_at_over_higher_id(self) -> None:
        conn = self._make_conn()
        confirmed_at = datetime(2026, 3, 9, 20, 0, tzinfo=UTC)
        conn.execute(
            """
            INSERT INTO twitch_live_state (
                twitch_user_id, streamer_login, is_live, last_started_at, last_game, active_session_id
            ) VALUES (?, ?, 1, ?, ?, ?)
            """,
            ("2002", "target_login", _iso_utc(confirmed_at - timedelta(hours=1)), "Deadlock", 77),
        )
        conn.execute(
            """
            INSERT INTO twitch_raid_history (
                id,
                from_broadcaster_id,
                from_broadcaster_login,
                to_broadcaster_id,
                to_broadcaster_login,
                executed_at,
                success
            ) VALUES (?, ?, ?, ?, ?, ?, 1)
            """,
            (99, "1001", "source_login", "2002", "target_login", _iso_utc(confirmed_at - timedelta(minutes=3))),
        )
        conn.execute(
            """
            INSERT INTO twitch_raid_history (
                id,
                from_broadcaster_id,
                from_broadcaster_login,
                to_broadcaster_id,
                to_broadcaster_login,
                executed_at,
                success
            ) VALUES (?, ?, ?, ?, ?, ?, 1)
            """,
            (10, "1001", "source_login", "2002", "target_login", _iso_utc(confirmed_at - timedelta(minutes=1))),
        )
        conn.commit()

        with patch(
            "bot.raid.partner_raid_score_tracking.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            tracking_id = track_confirmed_partner_raid(
                to_broadcaster_id="2002",
                to_broadcaster_login="target_login",
                from_broadcaster_login="source_login",
                from_broadcaster_id="1001",
                viewer_count=42,
                confirmed_at=confirmed_at,
            )

        self.assertIsNotNone(tracking_id)
        row = conn.execute(
            "SELECT raid_history_id, raid_history_executed_at FROM twitch_partner_raid_score_tracking WHERE id = ?",
            (tracking_id,),
        ).fetchone()
        self.assertIsNotNone(row)
        assert row is not None
        self.assertEqual(int(row["raid_history_id"]), 10)
        self.assertEqual(
            row["raid_history_executed_at"],
            _iso_utc(confirmed_at - timedelta(minutes=1)),
        )
        conn.close()

    def test_resolve_partner_raid_tracking_uses_first_non_deadlock_channel_update(self) -> None:
        conn = self._make_conn()
        confirmed_at = datetime(2026, 3, 9, 20, 0, tzinfo=UTC)
        ended_at = confirmed_at + timedelta(minutes=40)
        conn.execute(
            """
            INSERT INTO twitch_partner_raid_score_tracking (
                from_broadcaster_login,
                to_broadcaster_id,
                to_broadcaster_login,
                confirmed_at,
                target_session_id,
                was_deadlock_at_raid
            ) VALUES (?, ?, ?, ?, ?, 1)
            """,
            ("source_login", "2002", "target_login", _iso_utc(confirmed_at), 77),
        )
        conn.execute(
            """
            INSERT INTO twitch_channel_updates (twitch_user_id, game_name, recorded_at)
            VALUES (?, ?, ?)
            """,
            ("2002", "Just Chatting", _iso_utc(confirmed_at + timedelta(minutes=12))),
        )
        conn.commit()

        with patch(
            "bot.raid.partner_raid_score_tracking.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            resolved = resolve_partner_raid_tracking_for_session(
                twitch_user_id="2002",
                streamer_login="target_login",
                session_id=77,
                session_ended_at=ended_at,
            )

        self.assertEqual(resolved, 1)
        row = conn.execute(
            """
            SELECT deadlock_continued_sec, deadlock_continued_until, resolved_at, resolution_reason
            FROM twitch_partner_raid_score_tracking
            WHERE target_session_id = 77
            """
        ).fetchone()
        self.assertIsNotNone(row)
        assert row is not None
        self.assertEqual(int(row["deadlock_continued_sec"]), 12 * 60)
        self.assertEqual(row["resolution_reason"], "channel_update_non_deadlock")
        self.assertEqual(row["resolved_at"], _iso_utc(ended_at))
        self.assertEqual(
            row["deadlock_continued_until"],
            _iso_utc(confirmed_at + timedelta(minutes=12)),
        )
        conn.close()

    def test_resolve_partner_raid_tracking_falls_back_to_session_timing_when_session_id_missing(self) -> None:
        conn = self._make_conn()
        started_at = datetime(2026, 3, 9, 19, 0, tzinfo=UTC)
        confirmed_at = started_at + timedelta(minutes=5)
        ended_at = started_at + timedelta(minutes=40)
        conn.execute(
            """
            INSERT INTO twitch_stream_sessions (
                id, streamer_login, stream_id, started_at, ended_at, duration_seconds
            ) VALUES (?, ?, ?, ?, ?, ?)
            """,
            (77, "target_login", "stream-77", _iso_utc(started_at), _iso_utc(ended_at), 2400),
        )
        conn.execute(
            """
            INSERT INTO twitch_partner_raid_score_tracking (
                from_broadcaster_login,
                to_broadcaster_id,
                to_broadcaster_login,
                confirmed_at,
                target_session_id,
                target_stream_started_at,
                was_deadlock_at_raid
            ) VALUES (?, ?, ?, ?, NULL, ?, 1)
            """,
            ("source_login", "2002", "target_login", _iso_utc(confirmed_at), _iso_utc(started_at)),
        )
        conn.execute(
            """
            INSERT INTO twitch_channel_updates (twitch_user_id, game_name, recorded_at)
            VALUES (?, ?, ?)
            """,
            ("2002", "Just Chatting", _iso_utc(confirmed_at + timedelta(minutes=12))),
        )
        conn.commit()

        with patch(
            "bot.raid.partner_raid_score_tracking.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            resolved = resolve_partner_raid_tracking_for_session(
                twitch_user_id="2002",
                streamer_login="target_login",
                session_id=77,
                session_ended_at=ended_at,
            )

        self.assertEqual(resolved, 1)
        row = conn.execute(
            """
            SELECT deadlock_continued_sec, deadlock_continued_until, resolved_at, resolution_reason
            FROM twitch_partner_raid_score_tracking
            WHERE to_broadcaster_id = '2002'
            """
        ).fetchone()
        self.assertIsNotNone(row)
        assert row is not None
        self.assertEqual(int(row["deadlock_continued_sec"]), 12 * 60)
        self.assertEqual(row["resolution_reason"], "channel_update_non_deadlock")
        self.assertEqual(row["resolved_at"], _iso_utc(ended_at))
        conn.close()

    def test_resolve_partner_raid_tracking_merges_direct_and_fallback_rows_for_same_session(self) -> None:
        conn = self._make_conn()
        started_at = datetime(2026, 3, 9, 19, 0, tzinfo=UTC)
        confirmed_at = started_at + timedelta(minutes=5)
        ended_at = started_at + timedelta(minutes=40)
        conn.execute(
            """
            INSERT INTO twitch_stream_sessions (
                id, streamer_login, stream_id, started_at, ended_at, duration_seconds
            ) VALUES (?, ?, ?, ?, ?, ?)
            """,
            (77, "target_login", "stream-77", _iso_utc(started_at), _iso_utc(ended_at), 2400),
        )
        conn.execute(
            """
            INSERT INTO twitch_partner_raid_score_tracking (
                from_broadcaster_login,
                to_broadcaster_id,
                to_broadcaster_login,
                confirmed_at,
                target_session_id,
                was_deadlock_at_raid
            ) VALUES (?, ?, ?, ?, ?, 1)
            """,
            ("direct_source", "2002", "target_login", _iso_utc(confirmed_at), 77),
        )
        conn.execute(
            """
            INSERT INTO twitch_partner_raid_score_tracking (
                from_broadcaster_login,
                to_broadcaster_id,
                to_broadcaster_login,
                confirmed_at,
                target_session_id,
                target_stream_started_at,
                was_deadlock_at_raid
            ) VALUES (?, ?, ?, ?, NULL, ?, 1)
            """,
            (
                "fallback_source",
                "2002",
                "target_login",
                _iso_utc(confirmed_at + timedelta(minutes=1)),
                _iso_utc(started_at),
            ),
        )
        conn.execute(
            """
            INSERT INTO twitch_channel_updates (twitch_user_id, game_name, recorded_at)
            VALUES (?, ?, ?)
            """,
            ("2002", "Just Chatting", _iso_utc(confirmed_at + timedelta(minutes=12))),
        )
        conn.commit()

        with patch(
            "bot.raid.partner_raid_score_tracking.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            resolved = resolve_partner_raid_tracking_for_session(
                twitch_user_id="2002",
                streamer_login="target_login",
                session_id=77,
                session_ended_at=ended_at,
            )

        self.assertEqual(resolved, 2)
        rows = conn.execute(
            """
            SELECT deadlock_continued_until, resolved_at, resolution_reason
            FROM twitch_partner_raid_score_tracking
            WHERE to_broadcaster_id = '2002'
            ORDER BY id ASC
            """
        ).fetchall()
        self.assertEqual(len(rows), 2)
        for row in rows:
            self.assertEqual(row["resolution_reason"], "channel_update_non_deadlock")
            self.assertEqual(row["resolved_at"], _iso_utc(ended_at))
            self.assertEqual(
                row["deadlock_continued_until"],
                _iso_utc(confirmed_at + timedelta(minutes=12)),
            )
        conn.close()


class PartnerRaidScoreTrackingHookTests(unittest.IsolatedAsyncioTestCase):
    async def test_on_raid_arrival_tracks_confirmed_partner_raid_snapshot(self) -> None:
        conn = sqlite3.connect(":memory:")
        conn.row_factory = sqlite3.Row
        ensure_sqlite_twitch_schema(conn)
        conn.execute(
            """
            INSERT INTO twitch_streamers (twitch_login, twitch_user_id, silent_raid)
            VALUES (?, ?, 0)
            """,
            ("targetlogin", "9009"),
        )
        conn.commit()

        raid_bot = RaidBot.__new__(RaidBot)
        raid_bot._pending_raids = {
            "9009": (
                "source_login",
                {
                    "_partner_score": {
                        "final_score": 1.05,
                        "base_score": 0.7,
                    }
                },
                time.time(),
                True,
                42,
                None,
            )
        }
        raid_bot._refresh_partner_score_cache_if_available = AsyncMock()
        raid_bot._send_partner_raid_message = AsyncMock()
        raid_bot._send_recruitment_message_now = AsyncMock()

        try:
            with (
                patch(
                    "bot.raid.bot.readonly_connection",
                    side_effect=lambda: contextlib.nullcontext(_CompatConn(conn)),
                ),
                patch(
                    "bot.raid.bot.transaction",
                    side_effect=lambda: contextlib.nullcontext(_CompatConn(conn)),
                ),
                patch(
                    "bot.raid.bot.track_confirmed_partner_raid",
                    return_value=123,
                ) as track_mock,
            ):
                await raid_bot.on_raid_arrival(
                    to_broadcaster_id="9009",
                    to_broadcaster_login="targetlogin",
                    from_broadcaster_login="source_login",
                    from_broadcaster_id="source-id",
                    viewer_count=42,
                )
        finally:
            conn.close()

        track_mock.assert_called_once()
        kwargs = track_mock.call_args.kwargs
        self.assertEqual(kwargs["to_broadcaster_id"], "9009")
        self.assertEqual(kwargs["to_broadcaster_login"], "targetlogin")
        self.assertEqual(kwargs["viewer_count"], 42)
        self.assertEqual(kwargs["score_snapshot"]["final_score"], 1.05)

    async def test_finalize_stream_session_resolves_partner_raid_tracking(self) -> None:
        now_dt = datetime(2026, 3, 9, 21, 0, tzinfo=UTC)

        class _Harness(_SessionsMixin):
            def _get_active_sessions_cache(self):
                return {}

            def _lookup_open_session_id(self, login: str):
                return 77

            @staticmethod
            def _parse_dt(value):
                if isinstance(value, datetime):
                    return value
                text = str(value or "").replace("Z", "+00:00")
                parsed = datetime.fromisoformat(text)
                if parsed.tzinfo is None:
                    parsed = parsed.replace(tzinfo=UTC)
                return parsed

            @staticmethod
            def _get_target_game_lower():
                return "deadlock"

            async def _fetch_followers_total_safe(self, **kwargs):
                return None

        harness = _Harness()

        class _ConnContext:
            def __init__(self, conn):
                self._conn = conn

            def __enter__(self):
                return self._conn

            def __exit__(self, exc_type, exc, tb):
                return False

        class _FakeConn:
            def __init__(self):
                self.calls: list[tuple[str, tuple[object, ...]]] = []
                self.stage = 0

            def execute(self, sql, params=()):
                self.calls.append((sql, tuple(params or ())))
                if "SELECT * FROM twitch_stream_sessions WHERE id = %s" in sql:
                    self.stage = 1
                    return SimpleNamespace(
                        fetchone=lambda: {
                            "id": 77,
                            "started_at": "2026-03-09T19:00:00+00:00",
                            "start_viewers": 10,
                            "end_viewers": 10,
                            "peak_viewers": 10,
                            "avg_viewers": 10.0,
                            "samples": 0,
                            "followers_start": None,
                        }
                    )
                if "SELECT minutes_from_start, viewer_count FROM twitch_session_viewers" in sql:
                    return SimpleNamespace(fetchall=lambda: [])
                if "SELECT COUNT(*) AS uniq" in sql:
                    return SimpleNamespace(fetchone=lambda: {"uniq": 0, "firsts": 0})
                if "SELECT twitch_user_id, last_game, had_deadlock_in_session FROM twitch_live_state" in sql:
                    return SimpleNamespace(
                        fetchone=lambda: {
                            "twitch_user_id": "2002",
                            "last_game": "Deadlock",
                            "had_deadlock_in_session": 1,
                        }
                    )
                return SimpleNamespace(fetchall=lambda: [], fetchone=lambda: None)

        fake_conn = _FakeConn()

        with (
            patch(
                "bot.monitoring.sessions_mixin.storage.readonly_connection",
                side_effect=lambda: _ConnContext(fake_conn),
            ),
            patch(
                "bot.monitoring.sessions_mixin.storage.transaction",
                side_effect=lambda: _ConnContext(fake_conn),
            ),
            patch(
                "bot.monitoring.sessions_mixin.datetime",
                SimpleNamespace(now=lambda tz=UTC: now_dt),
            ),
            patch(
                "bot.monitoring.sessions_mixin.resolve_partner_raid_tracking_for_session",
                return_value=1,
            ) as resolve_mock,
        ):
            await harness._finalize_stream_session(login="targetlogin", reason="offline")

        resolve_mock.assert_called_once_with(
            twitch_user_id="2002",
            streamer_login="targetlogin",
            session_id=77,
            session_ended_at=now_dt,
        )


if __name__ == "__main__":
    unittest.main()
