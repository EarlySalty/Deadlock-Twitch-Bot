import contextlib
import sqlite3
import unittest
from datetime import UTC, datetime, timedelta
from unittest.mock import patch

from bot.community.partner_recruit import TwitchPartnerRecruitMixin


class _RecruitHarness(TwitchPartnerRecruitMixin):
    pass


class _CompatConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def execute(self, sql: str, params=None):
        sql_text = str(sql or "").replace("%s", "?")
        sql_text = sql_text.replace(
            "ts_utc > NOW() + (? || ' days')::interval",
            "ts_utc > datetime('now', ?)",
        )
        sql_text = sql_text.replace("cooldown_until > NOW()", "cooldown_until > datetime('now')")
        return self._conn.execute(sql_text, tuple(params or ()))

    def executemany(self, sql: str, params=None):
        sql_text = str(sql or "").replace("%s", "?")
        return self._conn.executemany(sql_text, params or ())

    def __getattr__(self, item):
        return getattr(self._conn, item)


class PartnerRecruitSamplingThresholdTests(unittest.TestCase):
    def _make_conn(self) -> sqlite3.Connection:
        conn = sqlite3.connect(":memory:")
        conn.row_factory = sqlite3.Row
        conn.execute("CREATE TABLE twitch_stats_category (streamer TEXT, ts_utc TEXT)")
        conn.execute("CREATE TABLE twitch_streamer_identities (twitch_login TEXT)")
        conn.execute(
            "CREATE TABLE twitch_partner_outreach (streamer_login TEXT, cooldown_until TEXT)"
        )
        conn.execute("CREATE TABLE twitch_raid_blacklist (target_login TEXT)")
        return conn

    def _insert_category_samples(
        self,
        conn: sqlite3.Connection,
        *,
        streamer: str,
        distinct_days: int,
        samples_per_day: int,
    ) -> None:
        base_day = datetime.now(UTC).replace(hour=12, minute=0, second=0, microsecond=0)
        for day_offset in range(distinct_days):
            day_start = base_day - timedelta(days=day_offset + 1)
            for sample_index in range(samples_per_day):
                sampled_at = day_start + timedelta(seconds=sample_index * 15)
                conn.execute(
                    "INSERT INTO twitch_stats_category (streamer, ts_utc) VALUES (?, ?)",
                    (streamer, sampled_at.strftime("%Y-%m-%d %H:%M:%S")),
                )

    def test_detect_recruit_candidates_requires_two_hours_per_day_at_15s_sampling(self) -> None:
        conn = self._make_conn()
        self._insert_category_samples(
            conn,
            streamer="candidate_under_threshold",
            distinct_days=4,
            samples_per_day=479,
        )
        conn.commit()

        with patch(
            "bot.community.partner_recruit.transaction",
            side_effect=lambda: contextlib.nullcontext(_CompatConn(conn)),
        ):
            candidates = _RecruitHarness()._detect_recruit_candidates()

        self.assertEqual(candidates, [])
        conn.close()

    def test_detect_recruit_candidates_accepts_four_days_with_480_samples_per_day(self) -> None:
        conn = self._make_conn()
        self._insert_category_samples(
            conn,
            streamer="candidate_over_threshold",
            distinct_days=4,
            samples_per_day=480,
        )
        conn.commit()

        with patch(
            "bot.community.partner_recruit.transaction",
            side_effect=lambda: contextlib.nullcontext(_CompatConn(conn)),
        ):
            candidates = _RecruitHarness()._detect_recruit_candidates()

        self.assertEqual(
            candidates,
            [{"streamer": "candidate_over_threshold", "distinct_days": 4}],
        )
        conn.close()


if __name__ == "__main__":
    unittest.main()
