import contextlib
import unittest
from unittest.mock import patch

from bot.core.constants import TWITCH_LOG_EVERY_N_TICKS
from bot.monitoring.monitoring import TwitchMonitoringMixin


class _PartnerStateConnection:
    def execute(self, sql: str, params=(), *args, **kwargs):
        return self

    def fetchall(self):
        return []


class _TrackedStreamerConnection:
    def __init__(self, rows) -> None:
        self._rows = list(rows)

    def execute(self, sql: str, params=(), *args, **kwargs):
        return self

    def fetchall(self):
        return list(self._rows)


class _ApiStub:
    def is_auth_blocked(self) -> bool:
        return False

    async def get_streams_by_logins(self, logins, language=None):
        return []

    async def get_streams_by_category(self, category_id, language=None, limit=100):
        return []


class _MonitoringHarness(TwitchMonitoringMixin):
    def __init__(self, log_every_n: int = TWITCH_LOG_EVERY_N_TICKS) -> None:
        self.api = _ApiStub()
        self._category_id = "509658"
        self._language_filters = []
        self._category_sample_limit = 50
        self._tick_count = 0
        self._log_every_n = log_every_n
        self.log_stats_calls = 0
        self.snapshot_reasons: list[str] = []
        self.reconciliation_triggers: list[str] = []

    async def _process_postings(self, tracked, streams_by_login) -> None:
        return None

    def _maybe_schedule_partner_raid_score_reconciliation(self, *, trigger: str) -> bool:
        self.reconciliation_triggers.append(trigger)
        return True

    async def _record_eventsub_capacity_snapshot(self, reason: str) -> None:
        self.snapshot_reasons.append(reason)

    async def _log_stats(self, streams_by_login: dict[str, dict], category_streams: list[dict]) -> None:
        self.log_stats_calls += 1

    async def _run_partner_recruit(self, category_streams: list[dict]) -> None:
        return None


class MonitoringStatsLoggingIntervalTests(unittest.IsolatedAsyncioTestCase):
    def test_load_tracked_streamers_includes_monitored_only_without_new_session_writer(self) -> None:
        harness = _MonitoringHarness()
        conn = _TrackedStreamerConnection(
            [
                {
                    "twitch_login": "partner_one",
                    "twitch_user_id": "100",
                    "require_discord_link": 1,
                    "archived_at": None,
                    "is_partner": 1,
                    "discord_user_id": "200",
                    "live_ping_role_id": 300,
                    "live_ping_enabled": 1,
                },
                {
                    "twitch_login": "revivelovesyou",
                    "twitch_user_id": "186111693",
                    "require_discord_link": 0,
                    "archived_at": None,
                    "is_partner": 0,
                    "discord_user_id": None,
                    "live_ping_role_id": None,
                    "live_ping_enabled": 1,
                },
            ]
        )

        with patch(
            "bot.monitoring.monitoring.storage.readonly_connection",
            side_effect=lambda: contextlib.nullcontext(conn),
        ), patch(
            "bot.monitoring.monitoring.storage.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            tracked, partner_logins = harness._load_tracked_streamers()

        self.assertEqual(
            [entry["login"] for entry in tracked],
            ["partner_one", "revivelovesyou"],
        )
        self.assertEqual(
            [bool(entry["is_verified"]) for entry in tracked],
            [True, False],
        )
        self.assertEqual(partner_logins, {"partner_one"})

    async def test_default_stats_logging_runs_every_poll_tick(self) -> None:
        harness = _MonitoringHarness()
        conn = _PartnerStateConnection()

        with patch(
            "bot.monitoring.monitoring.storage.readonly_connection",
            side_effect=lambda: contextlib.nullcontext(conn),
        ), patch(
            "bot.monitoring.monitoring.storage.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            await harness._tick()
            await harness._tick()

        self.assertEqual(harness.log_stats_calls, 2)
        self.assertEqual(harness._tick_count, 2)
        self.assertEqual(harness.snapshot_reasons, ["poll_tick", "poll_tick"])
        self.assertEqual(
            harness.reconciliation_triggers,
            ["poll_tick_reconciliation", "poll_tick_reconciliation"],
        )

    async def test_custom_tick_multiple_still_delays_stats_logging(self) -> None:
        harness = _MonitoringHarness(log_every_n=3)
        conn = _PartnerStateConnection()

        with patch(
            "bot.monitoring.monitoring.storage.readonly_connection",
            side_effect=lambda: contextlib.nullcontext(conn),
        ), patch(
            "bot.monitoring.monitoring.storage.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            await harness._tick()
            await harness._tick()
            self.assertEqual(harness.log_stats_calls, 0)
            await harness._tick()

        self.assertEqual(harness.log_stats_calls, 1)
        self.assertEqual(harness._tick_count, 3)


if __name__ == "__main__":
    unittest.main()
