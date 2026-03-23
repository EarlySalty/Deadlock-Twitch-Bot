import contextlib
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from bot.analytics.mixin import TwitchAnalyticsMixin


class _RecordingConnection:
    def __init__(self) -> None:
        self.executed: list[tuple[str, tuple[object, ...]]] = []

    def execute(self, sql: str, params=(), *args, **kwargs):
        self.executed.append((sql, tuple(params or ())))
        return self


class _AnalyticsCompatHarness(TwitchAnalyticsMixin):
    def __init__(self) -> None:
        self._analytics_observability_counter_store = {}
        self._chatters_scope_warned = set()
        self._chatters_user_fallback_warned = set()
        self._raid_bot = None
        self._twitch_chat_bot = None
        self._bot_token_manager = None


class AnalyticsApiResultCompatTests(unittest.IsolatedAsyncioTestCase):
    async def test_collect_subs_uses_legacy_api_when_structured_result_method_is_missing(self) -> None:
        harness = _AnalyticsCompatHarness()
        harness.api = SimpleNamespace(
            get_broadcaster_subscriptions=AsyncMock(
                return_value={"data": [{"tier": "1000"}], "total": 12, "points": 15}
            )
        )
        conn = _RecordingConnection()

        with (
            patch(
                "bot.analytics.mixin.storage.transaction",
                side_effect=lambda: contextlib.nullcontext(conn),
            ),
            patch("bot.analytics.mixin.storage.insert_observability_event"),
        ):
            ok = await harness._collect_subs_for_user("1001", "partner_one", "streamer-token")

        self.assertTrue(ok)
        harness.api.get_broadcaster_subscriptions.assert_awaited_once_with(
            "1001",
            "streamer-token",
        )
        self.assertTrue(any("INSERT INTO twitch_subscriptions_snapshot" in sql for sql, _ in conn.executed))

    async def test_collect_ads_uses_legacy_api_when_structured_result_method_is_missing(self) -> None:
        harness = _AnalyticsCompatHarness()
        harness.api = SimpleNamespace(
            get_ad_schedule=AsyncMock(
                return_value={
                    "next_ad_at": "2026-03-19T10:00:00+00:00",
                    "last_ad_at": "2026-03-19T09:30:00+00:00",
                    "duration": 90,
                    "preroll_free_time": 1200,
                    "snooze_count": 2,
                    "snooze_refresh_at": "2026-03-19T11:00:00+00:00",
                }
            )
        )
        conn = _RecordingConnection()

        with (
            patch(
                "bot.analytics.mixin.storage.transaction",
                side_effect=lambda: contextlib.nullcontext(conn),
            ),
            patch("bot.analytics.mixin.storage.insert_observability_event"),
        ):
            ok = await harness._collect_ads_schedule_for_user(
                "1001",
                "partner_one",
                "streamer-token",
            )

        self.assertTrue(ok)
        harness.api.get_ad_schedule.assert_awaited_once_with(
            "1001",
            "streamer-token",
        )
        self.assertTrue(any("INSERT INTO twitch_ads_schedule_snapshot" in sql for sql, _ in conn.executed))


if __name__ == "__main__":
    unittest.main()
