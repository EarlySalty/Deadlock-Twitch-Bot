from __future__ import annotations

import contextlib
import sqlite3
import unittest
from unittest.mock import AsyncMock

from bot.raid.services.candidate_selection import CandidateSelectionService

from tests.sqlite_twitch_schema import ensure_sqlite_twitch_schema


class _CompatConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    @staticmethod
    def _translate(sql: str) -> str:
        translated = str(sql).replace("NOW() - (%s::interval)", "datetime('now', ?)")
        translated = translated.replace("%s", "?")
        translated = translated.replace("NOW() - INTERVAL '1 day'", "datetime('now', '-1 day')")
        translated = translated.replace("NOW() - INTERVAL '7 days'", "datetime('now', '-7 days')")
        return translated

    def execute(self, sql: str, params=()):
        return self._conn.execute(self._translate(sql), params)

    def __getattr__(self, name: str):
        return getattr(self._conn, name)


class CandidateSelectionServiceTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.conn = sqlite3.connect(":memory:", check_same_thread=False)
        self.conn.row_factory = sqlite3.Row
        ensure_sqlite_twitch_schema(self.conn)

        self.service = CandidateSelectionService(
            load_partner_raid_score_map=lambda _user_ids: (_ for _ in ()).throw(RuntimeError("forced fallback")),
            recent_raid_targets_loader=lambda *_args: (_ for _ in ()).throw(RuntimeError("forced fallback")),
            readonly_connection_factory=lambda: contextlib.nullcontext(_CompatConn(self.conn)),
        )

    async def asyncTearDown(self) -> None:
        self.conn.close()

    def _score_loader(self, rows: dict[str, dict[str, object]]):
        def _loader(user_ids: list[str]) -> dict[str, dict[str, object]]:
            return {user_id: dict(rows[user_id]) for user_id in user_ids if user_id in rows}

        return _loader

    async def test_load_prepared_partner_scores_uses_db_fallback_and_computes_derived_scores(self) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_partner_raid_scores (
                twitch_user_id,
                twitch_login,
                is_live,
                duration_score,
                time_pattern_score,
                readiness_score,
                fairness_score,
                base_score,
                final_score,
                new_partner_multiplier,
                raid_boost_multiplier,
                today_received_raids,
                last_computed_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "1001",
                "Alpha",
                1,
                0.80,
                0.50,
                0.0,
                0.0,
                0.70,
                0.91,
                1.25,
                1.10,
                4,
                "2026-03-10T19:00:00+00:00",
            ),
        )

        scores = self.service.load_prepared_partner_scores(["1001"])

        self.assertIn("1001", scores)
        score = scores["1001"]
        self.assertEqual(score["twitch_login"], "alpha")
        self.assertTrue(score["is_live"])
        self.assertEqual(score["final_score"], 0.91)
        self.assertEqual(score["today_received_raids"], 4)
        self.assertAlmostEqual(score["readiness_score"], 0.68, places=2)
        self.assertAlmostEqual(score["fairness_score"], 0.74, places=2)
        self.assertEqual(score["new_partner_multiplier"], 1.25)
        self.assertEqual(score["raid_boost_multiplier"], 1.10)

    async def test_refresh_partner_score_cache_if_available_uses_injected_callback(self) -> None:
        refresh_mock = AsyncMock(return_value={"twitch_user_id": "1001"})
        service = CandidateSelectionService(
            refresh_partner_raid_score_async=refresh_mock,
            readonly_connection_factory=lambda: contextlib.nullcontext(_CompatConn(self.conn)),
        )

        await service.refresh_partner_score_cache_if_available(" 1001 ", reason="raid_confirmed")

        refresh_mock.assert_awaited_once_with("1001")

    async def test_get_recent_raid_targets_uses_injected_loader(self) -> None:
        service = CandidateSelectionService(
            recent_raid_targets_loader=lambda broadcaster_id, days: {"target-a"} if broadcaster_id == "source-1" and days == 7 else set(),
            readonly_connection_factory=lambda: contextlib.nullcontext(_CompatConn(self.conn)),
        )

        targets = service.get_recent_raid_targets("source-1", 7)

        self.assertEqual(targets, {"target-a"})

    async def test_select_partner_candidate_prefers_highest_final_score(self) -> None:
        service = CandidateSelectionService(
            load_partner_raid_score_map=self._score_loader(
                {
                    "1001": {"is_live": True, "final_score": 0.91, "today_received_raids": 5},
                    "2002": {"is_live": True, "final_score": 0.66, "today_received_raids": 0},
                }
            ),
            attach_followers_totals=AsyncMock(),
            readonly_connection_factory=lambda: contextlib.nullcontext(_CompatConn(self.conn)),
        )
        candidates = [
            {"user_id": "1001", "user_login": "alpha", "viewer_count": 50, "followers_total": 1000, "started_at": "2026-03-08T18:00:00+00:00"},
            {"user_id": "2002", "user_login": "bravo", "viewer_count": 10, "followers_total": 200, "started_at": "2026-03-08T18:10:00+00:00"},
        ]

        selected = await service.select_partner_candidate_by_score(candidates, "source-1")

        self.assertIsNotNone(selected)
        assert selected is not None
        self.assertEqual(selected["user_login"], "alpha")
        service.attach_followers_totals.assert_not_awaited()

    async def test_select_partner_candidate_uses_today_received_raids_for_close_scores(self) -> None:
        service = CandidateSelectionService(
            load_partner_raid_score_map=self._score_loader(
                {
                    "1001": {"is_live": True, "final_score": 0.90, "today_received_raids": 4},
                    "2002": {"is_live": True, "final_score": 0.86, "today_received_raids": 1},
                }
            ),
            attach_followers_totals=AsyncMock(),
            readonly_connection_factory=lambda: contextlib.nullcontext(_CompatConn(self.conn)),
        )
        candidates = [
            {"user_id": "1001", "user_login": "alpha", "viewer_count": 50, "followers_total": 1000, "started_at": "2026-03-08T18:00:00+00:00"},
            {"user_id": "2002", "user_login": "bravo", "viewer_count": 75, "followers_total": 800, "started_at": "2026-03-08T17:00:00+00:00"},
        ]

        selected = await service.select_partner_candidate_by_score(candidates, "source-2")

        self.assertIsNotNone(selected)
        assert selected is not None
        self.assertEqual(selected["user_login"], "bravo")
        service.attach_followers_totals.assert_not_awaited()

    async def test_select_partner_candidate_falls_back_to_viewers_followers_started_at(self) -> None:
        service = CandidateSelectionService(
            load_partner_raid_score_map=self._score_loader(
                {
                    "1001": {"is_live": True, "final_score": 0.90, "today_received_raids": 1},
                    "2002": {"is_live": True, "final_score": 0.86, "today_received_raids": 1},
                    "3003": {"is_live": True, "final_score": 0.87, "today_received_raids": 1},
                }
            ),
            attach_followers_totals=AsyncMock(),
            readonly_connection_factory=lambda: contextlib.nullcontext(_CompatConn(self.conn)),
        )
        candidates = [
            {"user_id": "1001", "user_login": "alpha", "viewer_count": 40, "followers_total": 500, "started_at": "2026-03-08T18:00:00+00:00"},
            {"user_id": "2002", "user_login": "bravo", "viewer_count": 10, "followers_total": 800, "started_at": "2026-03-08T17:00:00+00:00"},
            {"user_id": "3003", "user_login": "charlie", "viewer_count": 10, "followers_total": 300, "started_at": "2026-03-08T16:00:00+00:00"},
        ]

        selected = await service.select_partner_candidate_by_score(candidates, "source-3")

        self.assertIsNotNone(selected)
        assert selected is not None
        self.assertEqual(selected["user_login"], "charlie")
        service.attach_followers_totals.assert_awaited_once()

    async def test_select_fairest_candidate_keeps_recent_target_cooldown_for_fallback(self) -> None:
        service = CandidateSelectionService(
            recent_raid_targets_loader=lambda *_args: {"1001"},
            attach_followers_totals=AsyncMock(),
            readonly_connection_factory=lambda: contextlib.nullcontext(_CompatConn(self.conn)),
        )
        candidates = [
            {"user_id": "1001", "user_login": "alpha", "viewer_count": 10, "followers_total": 100, "started_at": "2026-03-08T18:00:00+00:00"},
            {"user_id": "2002", "user_login": "bravo", "viewer_count": 25, "followers_total": 100, "started_at": "2026-03-08T17:00:00+00:00"},
        ]

        selected = await service.select_fairest_candidate(candidates, "source-4")

        self.assertIsNotNone(selected)
        assert selected is not None
        self.assertEqual(selected["user_login"], "bravo")
        service.attach_followers_totals.assert_awaited_once()


if __name__ == "__main__":
    unittest.main()
