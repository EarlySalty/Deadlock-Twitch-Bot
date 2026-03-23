import contextlib
import sqlite3
import unittest
from datetime import UTC, datetime, timedelta

from bot.raid.partner_scores import BERLIN_TZ, PartnerRaidScoreService

from tests.sqlite_twitch_schema import ensure_sqlite_twitch_schema


def _iso_utc(value: datetime) -> str:
    return value.astimezone(UTC).isoformat(timespec="seconds")


class PartnerRaidScoreCacheTests(unittest.TestCase):
    def _make_conn(self) -> sqlite3.Connection:
        conn = sqlite3.connect(":memory:")
        conn.row_factory = sqlite3.Row
        ensure_sqlite_twitch_schema(conn)
        return conn

    def _make_service(self, conn: sqlite3.Connection) -> PartnerRaidScoreService:
        return PartnerRaidScoreService(lambda: contextlib.nullcontext(conn))

    def _insert_partner(self, conn: sqlite3.Connection, login: str, user_id: str) -> None:
        conn.execute(
            """
            INSERT INTO twitch_streamer_identities (
                twitch_login,
                twitch_user_id
            ) VALUES (?, ?)
            ON CONFLICT(twitch_user_id) DO UPDATE SET
                twitch_login = excluded.twitch_login
            """,
            (login, user_id),
        )
        conn.execute(
            """
            INSERT INTO twitch_partners (
                twitch_login,
                twitch_user_id,
                manual_verified_at,
                raid_bot_enabled,
                partnered_at,
                status
            ) VALUES (?, ?, ?, 1, ?, 'active')
            """,
            (
                login,
                user_id,
                "2026-03-01T10:00:00+00:00",
                "2026-03-01T10:00:00+00:00",
            ),
        )

    def _insert_session(
        self,
        conn: sqlite3.Connection,
        *,
        login: str,
        started_at: datetime,
        duration_seconds: int,
    ) -> None:
        ended_at = started_at + timedelta(seconds=duration_seconds)
        conn.execute(
            """
            INSERT INTO twitch_stream_sessions (
                streamer_login,
                stream_id,
                started_at,
                ended_at,
                duration_seconds
            ) VALUES (?, ?, ?, ?, ?)
            """,
            (
                login,
                f"stream-{started_at.timestamp()}",
                _iso_utc(started_at),
                _iso_utc(ended_at),
                duration_seconds,
            ),
        )

    def test_refresh_partner_score_persists_live_row_with_expected_scores(self) -> None:
        conn = self._make_conn()
        service = self._make_service(conn)
        now = datetime(2026, 3, 8, 18, 0, tzinfo=UTC)

        self._insert_partner(conn, "alpha", "1001")
        conn.execute(
            """
            INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_started_at)
            VALUES (?, ?, 1, ?)
            """,
            ("1001", "alpha", _iso_utc(now - timedelta(hours=1))),
        )
        conn.execute(
            """
            INSERT INTO streamer_plans (twitch_user_id, twitch_login, raid_boost_enabled)
            VALUES (?, ?, 1)
            """,
            ("1001", "alpha"),
        )

        matching_bucket = datetime(2026, 3, 1, 19, 0, tzinfo=BERLIN_TZ).astimezone(UTC)
        self._insert_session(conn, login="alpha", started_at=matching_bucket, duration_seconds=7200)
        self._insert_session(
            conn,
            login="alpha",
            started_at=matching_bucket - timedelta(days=7),
            duration_seconds=7200,
        )
        self._insert_session(
            conn,
            login="alpha",
            started_at=matching_bucket - timedelta(days=14),
            duration_seconds=7200,
        )
        self._insert_session(
            conn,
            login="alpha",
            started_at=datetime(2026, 2, 26, 20, 0, tzinfo=BERLIN_TZ).astimezone(UTC),
            duration_seconds=7200,
        )

        conn.execute(
            """
            INSERT INTO twitch_raid_history (
                from_broadcaster_id,
                from_broadcaster_login,
                to_broadcaster_id,
                to_broadcaster_login,
                executed_at,
                success
            ) VALUES (?, ?, ?, ?, ?, 1)
            """,
            ("s1", "sender_one", "1001", "alpha", _iso_utc(now - timedelta(hours=2))),
        )
        conn.execute(
            """
            INSERT INTO twitch_raid_history (
                from_broadcaster_id,
                from_broadcaster_login,
                to_broadcaster_id,
                to_broadcaster_login,
                executed_at,
                success
            ) VALUES (?, ?, ?, ?, ?, 1)
            """,
            ("s2", "sender_two", "1001", "alpha", _iso_utc(now - timedelta(days=1))),
        )
        conn.execute(
            """
            INSERT INTO twitch_raid_history (
                from_broadcaster_id,
                from_broadcaster_login,
                to_broadcaster_id,
                to_broadcaster_login,
                executed_at,
                success
            ) VALUES (?, ?, ?, ?, ?, ?)
            """,
            ("s3", "sender_three", "1001", "alpha", _iso_utc(now - timedelta(minutes=30)), False),
        )

        row = service.refresh_partner_score("1001", now=now)

        self.assertIsNotNone(row)
        assert row is not None
        self.assertTrue(row["is_live"])
        self.assertEqual(row["avg_duration_sec"], 7200)
        self.assertAlmostEqual(row["duration_score"], 0.5, places=6)
        self.assertAlmostEqual(row["time_pattern_score"], 0.75, places=6)
        self.assertAlmostEqual(row["base_score"], 0.625, places=6)
        self.assertAlmostEqual(row["new_partner_multiplier"], 1.2, places=6)
        self.assertAlmostEqual(row["raid_boost_multiplier"], 1.5, places=6)
        self.assertAlmostEqual(row["final_score"], 1.125, places=6)
        self.assertEqual(row["received_successful_raids_total"], 2)
        self.assertEqual(row["today_received_raids"], 1)
        self.assertTrue(row["is_new_partner_preferred"])

        cached = conn.execute(
            "SELECT final_score, is_live FROM twitch_partner_raid_scores WHERE twitch_user_id = ?",
            ("1001",),
        ).fetchone()
        self.assertIsNotNone(cached)
        self.assertAlmostEqual(float(cached["final_score"]), 1.125, places=6)
        self.assertEqual(int(cached["is_live"]), 1)
        conn.close()

    def test_refresh_partner_score_keeps_offline_snapshot(self) -> None:
        conn = self._make_conn()
        service = self._make_service(conn)
        now = datetime(2026, 3, 8, 18, 0, tzinfo=UTC)

        self._insert_partner(conn, "bravo", "2002")
        conn.execute(
            """
            INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_started_at)
            VALUES (?, ?, 1, ?)
            """,
            ("2002", "bravo", _iso_utc(now - timedelta(hours=1))),
        )
        matching_bucket = datetime(2026, 3, 1, 19, 0, tzinfo=BERLIN_TZ).astimezone(UTC)
        for weeks_ago in (0, 1, 2):
            self._insert_session(
                conn,
                login="bravo",
                started_at=matching_bucket - timedelta(days=7 * weeks_ago),
                duration_seconds=5400,
            )

        first = service.refresh_partner_score("2002", now=now)
        self.assertIsNotNone(first)
        assert first is not None

        conn.execute(
            "UPDATE twitch_live_state SET is_live = 0 WHERE twitch_user_id = ?",
            ("2002",),
        )
        conn.execute(
            """
            INSERT INTO twitch_raid_history (
                from_broadcaster_id,
                from_broadcaster_login,
                to_broadcaster_id,
                to_broadcaster_login,
                executed_at,
                success
            ) VALUES (?, ?, ?, ?, ?, 1)
            """,
            ("sender", "sender", "2002", "bravo", _iso_utc(now),),
        )

        refreshed = service.refresh_partner_score("2002", now=now + timedelta(minutes=5))

        self.assertIsNotNone(refreshed)
        assert refreshed is not None
        self.assertFalse(refreshed["is_live"])
        self.assertEqual(refreshed["received_successful_raids_total"], 1)
        self.assertAlmostEqual(refreshed["final_score"], float(first["final_score"]), places=6)
        self.assertEqual(refreshed["current_started_at"], first["current_started_at"])
        conn.close()

    def test_refresh_all_partner_scores_and_loader_filter_live_only(self) -> None:
        conn = self._make_conn()
        service = self._make_service(conn)
        now = datetime(2026, 3, 8, 18, 0, tzinfo=UTC)

        self._insert_partner(conn, "charlie", "3003")
        self._insert_partner(conn, "delta", "4004")
        conn.execute(
            """
            INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_started_at)
            VALUES (?, ?, 1, ?)
            """,
            ("3003", "charlie", _iso_utc(now - timedelta(minutes=30))),
        )
        conn.execute(
            """
            INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_started_at)
            VALUES (?, ?, 0, ?)
            """,
            ("4004", "delta", _iso_utc(now - timedelta(hours=3))),
        )
        self._insert_session(
            conn,
            login="charlie",
            started_at=datetime(2026, 3, 1, 19, 0, tzinfo=BERLIN_TZ).astimezone(UTC),
            duration_seconds=3600,
        )
        self._insert_session(
            conn,
            login="charlie",
            started_at=datetime(2026, 2, 22, 19, 0, tzinfo=BERLIN_TZ).astimezone(UTC),
            duration_seconds=3600,
        )
        self._insert_session(
            conn,
            login="charlie",
            started_at=datetime(2026, 2, 15, 19, 0, tzinfo=BERLIN_TZ).astimezone(UTC),
            duration_seconds=3600,
        )

        refreshed = service.refresh_all_partner_scores(now=now)

        self.assertEqual(set(refreshed), {"3003", "4004"})
        self.assertTrue(refreshed["3003"]["is_live"])
        self.assertFalse(refreshed["4004"]["is_live"])
        self.assertAlmostEqual(refreshed["4004"]["duration_score"], 0.5, places=6)
        self.assertAlmostEqual(refreshed["4004"]["time_pattern_score"], 0.5, places=6)
        self.assertAlmostEqual(refreshed["4004"]["new_partner_multiplier"], 1.25, places=6)
        self.assertAlmostEqual(refreshed["4004"]["final_score"], 0.625, places=6)

        loaded_live_only = service.load_scores(["3003", "4004"], live_only=True)
        self.assertEqual(set(loaded_live_only), {"3003"})
        conn.close()

    def test_refresh_partner_score_treats_billing_bundle_plan_as_raid_boost(self) -> None:
        conn = self._make_conn()
        service = self._make_service(conn)
        now = datetime(2026, 3, 8, 18, 0, tzinfo=UTC)

        self._insert_partner(conn, "echo", "5005")
        conn.execute(
            """
            INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_started_at)
            VALUES (?, ?, 1, ?)
            """,
            ("5005", "echo", _iso_utc(now - timedelta(minutes=30))),
        )
        conn.execute(
            """
            INSERT INTO streamer_plans (twitch_user_id, twitch_login, plan_name)
            VALUES (?, ?, ?)
            """,
            ("5005", "echo", "bundle"),
        )
        matching_bucket = datetime(2026, 3, 1, 19, 0, tzinfo=BERLIN_TZ).astimezone(UTC)
        for weeks_ago in (0, 1, 2):
            self._insert_session(
                conn,
                login="echo",
                started_at=matching_bucket - timedelta(days=7 * weeks_ago),
                duration_seconds=3600,
            )

        row = service.refresh_partner_score("5005", now=now)

        self.assertIsNotNone(row)
        assert row is not None
        self.assertAlmostEqual(row["raid_boost_multiplier"], 1.5, places=6)
        self.assertGreater(float(row["final_score"]), float(row["base_score"]))
        conn.close()

    def test_refresh_partner_score_treats_active_manual_bundle_override_as_raid_boost(self) -> None:
        conn = self._make_conn()
        service = self._make_service(conn)
        now = datetime(2026, 3, 8, 18, 0, tzinfo=UTC)

        self._insert_partner(conn, "foxtrot", "6006")
        conn.execute(
            """
            INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_started_at)
            VALUES (?, ?, 1, ?)
            """,
            ("6006", "foxtrot", _iso_utc(now - timedelta(minutes=25))),
        )
        conn.execute(
            """
            INSERT INTO streamer_plans (
                twitch_user_id, twitch_login, plan_name, manual_plan_id, manual_plan_expires_at
            ) VALUES (?, ?, ?, ?, ?)
            """,
            (
                "6006",
                "foxtrot",
                "free",
                "bundle_analysis_raid_boost",
                _iso_utc(now + timedelta(days=2)),
            ),
        )
        matching_bucket = datetime(2026, 3, 1, 20, 0, tzinfo=BERLIN_TZ).astimezone(UTC)
        for weeks_ago in (0, 1, 2):
            self._insert_session(
                conn,
                login="foxtrot",
                started_at=matching_bucket - timedelta(days=7 * weeks_ago),
                duration_seconds=3600,
            )

        row = service.refresh_partner_score("6006", now=now)

        self.assertIsNotNone(row)
        assert row is not None
        self.assertAlmostEqual(row["raid_boost_multiplier"], 1.5, places=6)
        conn.close()


if __name__ == "__main__":
    unittest.main()
