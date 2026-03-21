import asyncio
import contextlib
import unittest
from unittest.mock import AsyncMock, patch

from bot.analytics.mixin import TwitchAnalyticsMixin
from bot.dashboard.billing.billing_mixin import _DashboardBillingMixin
from bot.monitoring.eventsub_mixin import _EventSubMixin
from bot.monitoring.monitoring import TwitchMonitoringMixin


class _RecordingConnection:
    def __init__(self) -> None:
        self.executed: list[tuple[str, tuple[object, ...]]] = []

    def execute(self, sql: str, params=(), *args, **kwargs):
        self.executed.append((sql, tuple(params or ())))
        return self

    def fetchone(self):
        return None

    def fetchall(self):
        return []


class _AnalyticsHarness(TwitchAnalyticsMixin, TwitchMonitoringMixin):
    def __init__(self) -> None:
        self.live_events: list[tuple[str, str]] = []
        self.refresh_events: list[dict[str, object]] = []

    async def _handle_stream_went_live(self, broadcaster_user_id: str, broadcaster_login: str) -> None:
        self.live_events.append((broadcaster_user_id, broadcaster_login))

    async def _request_partner_raid_score_refresh(self, **kwargs):
        self.refresh_events.append(dict(kwargs))
        return True


class _MonitoringHarness(TwitchMonitoringMixin):
    def __init__(self) -> None:
        self.calls: list[dict[str, object]] = []
        self.spawned: list[str] = []

    def _spawn_bg_task(self, coro, name: str) -> None:
        self.spawned.append(name)
        import asyncio

        asyncio.create_task(coro, name=name)

    async def refresh_partner_raid_score_cache(
        self,
        *,
        twitch_user_id: str | None = None,
        login: str | None = None,
        trigger: str,
        full_refresh: bool = False,
        immediate: bool = True,
    ) -> None:
        self.calls.append(
            {
                "twitch_user_id": twitch_user_id,
                "login": login,
                "trigger": trigger,
                "full_refresh": full_refresh,
                "immediate": immediate,
            }
        )


class _MonitoringDispatchHarness(TwitchMonitoringMixin):
    def __init__(self, service: object) -> None:
        self.partner_raid_score_service = service


class _AsyncPreferredService:
    def __init__(self) -> None:
        self.sync_calls: list[dict[str, object]] = []
        self.async_calls: list[dict[str, object]] = []

    def refresh_partner_raid_score(
        self,
        *,
        twitch_user_id: str | None = None,
        login: str | None = None,
        trigger: str,
    ):
        self.sync_calls.append(
            {
                "twitch_user_id": twitch_user_id,
                "login": login,
                "trigger": trigger,
            }
        )
        return {"mode": "sync"}

    async def refresh_partner_raid_score_async(
        self,
        *,
        twitch_user_id: str | None = None,
        login: str | None = None,
        trigger: str,
    ):
        self.async_calls.append(
            {
                "twitch_user_id": twitch_user_id,
                "login": login,
                "trigger": trigger,
            }
        )
        return {"mode": "async"}


class _BrokenRefreshService:
    def __init__(self) -> None:
        self.calls: list[dict[str, object]] = []

    def refresh_partner_raid_score(
        self,
        *,
        twitch_user_id: str | None = None,
        login: str | None = None,
        trigger: str,
    ):
        self.calls.append(
            {
                "twitch_user_id": twitch_user_id,
                "login": login,
                "trigger": trigger,
            }
        )
        raise TypeError("internal refresh bug")


class _EventSubHarness(_EventSubMixin):
    def __init__(self) -> None:
        self.offline_calls: list[dict[str, object]] = []
        self.refresh_events: list[dict[str, object]] = []

    def _load_live_state_row(self, login_lower: str) -> dict:
        return {
            "is_live": 1,
            "last_game": "Deadlock",
            "had_deadlock_in_session": 1,
            "last_started_at": "2026-03-08T12:00:00+00:00",
        }

    def _get_tracked_logins_for_eventsub(self) -> list[str]:
        return []

    async def _fetch_streams_by_logins_quick(self, tracked_logins: list[str]) -> dict:
        return {}

    async def _handle_auto_raid_on_offline(self, **kwargs) -> None:
        self.offline_calls.append(dict(kwargs))

    async def _request_partner_raid_score_refresh(self, **kwargs):
        self.refresh_events.append(dict(kwargs))
        return True


class _BillingHarness(_DashboardBillingMixin):
    pass


class PartnerRaidScoreRefreshTriggerTests(unittest.IsolatedAsyncioTestCase):
    async def test_stream_online_updates_live_state_and_requests_refresh(self) -> None:
        harness = _AnalyticsHarness()
        conn = _RecordingConnection()

        with patch(
            "bot.analytics.mixin.storage.get_conn",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            await harness._handle_stream_online(
                "1234",
                "partner_one",
                {"id": "stream-1", "started_at": "2026-03-08T12:00:00+00:00"},
            )

        self.assertEqual(harness.live_events, [("1234", "partner_one")])
        self.assertEqual(
            harness.refresh_events,
            [
                {
                    "twitch_user_id": "1234",
                    "login": "partner_one",
                    "trigger": "eventsub_stream_online",
                }
            ],
        )
        self.assertTrue(any("INSERT INTO twitch_live_state" in sql for sql, _ in conn.executed))

    async def test_channel_update_requests_refresh_after_db_update(self) -> None:
        harness = _AnalyticsHarness()
        conn = _RecordingConnection()

        with patch(
            "bot.analytics.mixin.storage.get_conn",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            await harness._handle_channel_update(
                "2222",
                {
                    "title": "Fresh title",
                    "category_name": "Deadlock",
                    "broadcaster_language": "de",
                },
            )

        self.assertEqual(
            harness.refresh_events,
            [
                {
                    "twitch_user_id": "2222",
                    "trigger": "eventsub_channel_update",
                }
            ],
        )
        self.assertEqual(len(conn.executed), 2)
        self.assertIn("INSERT INTO twitch_channel_updates", conn.executed[0][0])
        self.assertIn("UPDATE twitch_live_state", conn.executed[1][0])

    async def test_request_partner_raid_score_refresh_uses_available_service(self) -> None:
        harness = _MonitoringHarness()

        ok = await harness._request_partner_raid_score_refresh(
            twitch_user_id="9999",
            login="partner_x",
            trigger="unit_test",
        )

        self.assertTrue(ok)
        self.assertEqual(
            harness.calls,
            [
                {
                    "twitch_user_id": "9999",
                    "login": "partner_x",
                    "trigger": "unit_test",
                    "full_refresh": False,
                    "immediate": True,
                }
            ],
        )

    async def test_request_partner_raid_score_refresh_prefers_async_wrapper(self) -> None:
        service = _AsyncPreferredService()
        harness = _MonitoringDispatchHarness(service)

        ok = await harness._request_partner_raid_score_refresh(
            twitch_user_id="9999",
            login="partner_x",
            trigger="unit_test",
        )

        self.assertTrue(ok)
        self.assertEqual(service.sync_calls, [])
        self.assertEqual(
            service.async_calls,
            [
                {
                    "twitch_user_id": "9999",
                    "login": "partner_x",
                    "trigger": "unit_test",
                }
            ],
        )

    async def test_request_partner_raid_score_refresh_logs_internal_type_error(self) -> None:
        service = _BrokenRefreshService()
        harness = _MonitoringDispatchHarness(service)

        with patch("bot.monitoring.monitoring.log.exception") as log_exception:
            ok = await harness._request_partner_raid_score_refresh(
                twitch_user_id="9999",
                login="partner_x",
                trigger="unit_test",
            )

        self.assertFalse(ok)
        self.assertEqual(
            service.calls,
            [
                {
                    "twitch_user_id": "9999",
                    "login": "partner_x",
                    "trigger": "unit_test",
                }
            ],
        )
        log_exception.assert_called()

    async def test_billing_refresh_uses_async_wrapper_on_running_loop(self) -> None:
        harness = _BillingHarness()

        with (
            patch(
                "bot.dashboard.billing.billing_mixin.refresh_partner_raid_score_async",
                new=AsyncMock(return_value={"twitch_user_id": "9009"}),
            ) as async_refresh,
            patch(
                "bot.dashboard.billing.billing_mixin.refresh_partner_raid_score",
                side_effect=AssertionError("sync refresh should not be called"),
            ),
        ):
            harness._billing_refresh_partner_raid_score_cache(
                twitch_user_id="9009",
                twitch_login="partner_x",
                reason="billing_test",
            )
            await asyncio.sleep(0)

        async_refresh.assert_awaited_once_with("9009")

    async def test_schedule_partner_raid_score_refreshes_deduplicates_targets(self) -> None:
        harness = _MonitoringHarness()
        scheduled_calls: list[dict[str, object]] = []

        def _record_schedule(**kwargs):
            scheduled_calls.append(dict(kwargs))
            return True

        harness._schedule_partner_raid_score_refresh = _record_schedule  # type: ignore[method-assign]

        scheduled = harness._schedule_partner_raid_score_refreshes(
            [
                ("123", "partner_one", "poll_stream_online"),
                ("123", "partner_one", "poll_stream_restarted"),
                ("456", "partner_two", "poll_stream_offline"),
            ]
        )

        self.assertEqual(scheduled, 2)
        self.assertEqual(
            scheduled_calls,
            [
                {
                    "twitch_user_id": "123",
                    "login": "partner_one",
                    "trigger": "poll_stream_online",
                },
                {
                    "twitch_user_id": "456",
                    "login": "partner_two",
                    "trigger": "poll_stream_offline",
                },
            ],
        )

    async def test_reconciliation_runs_at_most_every_five_minutes(self) -> None:
        harness = _MonitoringHarness()
        scheduled_calls: list[dict[str, object]] = []

        def _record_schedule(**kwargs):
            scheduled_calls.append(dict(kwargs))
            return True

        harness._schedule_partner_raid_score_refresh = _record_schedule  # type: ignore[method-assign]

        with patch("bot.monitoring.monitoring.time.monotonic", side_effect=[100.0, 200.0, 401.0]):
            first = harness._maybe_schedule_partner_raid_score_reconciliation(
                trigger="poll_tick_reconciliation"
            )
            second = harness._maybe_schedule_partner_raid_score_reconciliation(
                trigger="poll_tick_reconciliation"
            )
            third = harness._maybe_schedule_partner_raid_score_reconciliation(
                trigger="poll_tick_reconciliation"
            )

        self.assertTrue(first)
        self.assertFalse(second)
        self.assertTrue(third)
        self.assertEqual(
            scheduled_calls,
            [
                {"trigger": "poll_tick_reconciliation", "full_refresh": True},
                {"trigger": "poll_tick_reconciliation", "full_refresh": True},
            ],
        )

    async def test_eventsub_stream_offline_updates_live_state_and_requests_refresh(self) -> None:
        harness = _EventSubHarness()
        conn = _RecordingConnection()

        with patch(
            "bot.monitoring.eventsub_mixin.storage.get_conn",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            await harness._on_eventsub_stream_offline("1234", "partner_one")

        self.assertEqual(len(harness.offline_calls), 1)
        self.assertEqual(
            harness.refresh_events,
            [
                {
                    "twitch_user_id": "1234",
                    "login": "partner_one",
                    "trigger": "eventsub_stream_offline",
                }
            ],
        )
        live_state_updates = [sql for sql, _ in conn.executed if "UPDATE twitch_live_state" in sql]
        self.assertTrue(live_state_updates)
        self.assertIn("active_session_id = NULL", live_state_updates[0])


if __name__ == "__main__":
    unittest.main()
