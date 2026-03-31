import asyncio
import contextlib
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from bot.analytics.mixin import TwitchAnalyticsMixin
from bot.dashboard.billing.billing_mixin import _DashboardBillingMixin
from bot.monitoring.eventsub_mixin import _EventSubMixin
from bot.monitoring.eventsub_state_store import EventSubStateStore
from bot.monitoring.monitoring import TwitchMonitoringMixin
from tests.eventsub_state_store_test_helpers import InMemoryEventSubStateRepository


class _RecordingConnection:
    def __init__(
        self,
        *,
        fetchall_rows: list[dict[str, object]] | None = None,
        fetchall_rows_by_sql: dict[str, list[dict[str, object]]] | None = None,
    ) -> None:
        self.executed: list[tuple[str, tuple[object, ...]]] = []
        self.fetchall_rows = list(fetchall_rows or [])
        self.fetchall_rows_by_sql = {
            pattern: list(rows) for pattern, rows in (fetchall_rows_by_sql or {}).items()
        }

    def execute(self, sql: str, params=(), *args, **kwargs):
        self.executed.append((sql, tuple(params or ())))
        return self

    def fetchone(self):
        return None

    def fetchall(self):
        if self.executed and self.fetchall_rows_by_sql:
            last_sql = self.executed[-1][0]
            for pattern, rows in self.fetchall_rows_by_sql.items():
                if pattern in last_sql:
                    return list(rows)
        return list(self.fetchall_rows)


class _AnalyticsHarness(TwitchAnalyticsMixin, TwitchMonitoringMixin):
    def __init__(self) -> None:
        self.live_events: list[tuple[str, str]] = []
        self.refresh_events: list[dict[str, object]] = []

    async def _handle_stream_went_live(self, broadcaster_user_id: str, broadcaster_login: str) -> None:
        self.live_events.append((broadcaster_user_id, broadcaster_login))

    async def _request_partner_raid_score_refresh(self, **kwargs):
        self.refresh_events.append(dict(kwargs))
        return True


class _AnalyticsScheduledHarness(TwitchAnalyticsMixin, TwitchMonitoringMixin):
    def __init__(self) -> None:
        self.scheduled_refreshes: list[dict[str, object]] = []

    def _spawn_bg_task(self, coro, name: str):
        asyncio.create_task(coro, name=name)
        return object()

    def _schedule_partner_raid_score_refresh(self, **kwargs):
        self.scheduled_refreshes.append(dict(kwargs))
        return True


class _AnalyticsDeferredFollowupHarness(TwitchAnalyticsMixin, TwitchMonitoringMixin):
    def __init__(self) -> None:
        self.live_events: list[tuple[str, str]] = []
        self.refresh_events: list[dict[str, object]] = []
        self.enqueued_followups: list[dict[str, object]] = []
        self._eventsub_defer_stream_online_followups = True

    async def _handle_stream_went_live(self, broadcaster_user_id: str, broadcaster_login: str) -> None:
        self.live_events.append((broadcaster_user_id, broadcaster_login))

    async def _request_partner_raid_score_refresh(self, **kwargs):
        self.refresh_events.append(dict(kwargs))
        return True

    async def _enqueue_eventsub_stream_online_followups_processing(self, **kwargs):
        self.enqueued_followups.append(dict(kwargs))


class _IdempotentAnalyticsHarness(TwitchAnalyticsMixin, TwitchMonitoringMixin):
    def __init__(self) -> None:
        self.live_events: list[tuple[str, str]] = []
        self.refresh_events: list[dict[str, object]] = []
        self._eventsub_state_store = EventSubStateStore(
            repository=InMemoryEventSubStateRepository(),
        )

    def _get_eventsub_state_store(self) -> EventSubStateStore:
        return self._eventsub_state_store

    async def _run_eventsub_business_effect_once(
        self,
        *,
        message_id: str | None,
        effect_name: str,
        coro_factory,
        ttl_seconds: float = 7 * 24 * 3600.0,
    ) -> bool:
        normalized_message_id = str(message_id or "").strip()
        normalized_effect_name = str(effect_name or "").strip().lower()
        if not normalized_message_id:
            await coro_factory()
            return True
        guard_key = f"{normalized_effect_name}:{normalized_message_id}"
        claimed = self._get_eventsub_state_store().claim(
            "business_effect",
            guard_key,
            ttl_seconds=ttl_seconds,
        )
        if not claimed:
            return False
        try:
            await coro_factory()
        except Exception:
            self._get_eventsub_state_store().release("business_effect", guard_key)
            raise
        return True

    async def _handle_stream_went_live(self, broadcaster_user_id: str, broadcaster_login: str) -> None:
        self.live_events.append((broadcaster_user_id, broadcaster_login))

    async def _request_partner_raid_score_refresh(self, **kwargs):
        self.refresh_events.append(dict(kwargs))
        return True


class _IdempotentOfflineHarness(_EventSubMixin):
    def __init__(self) -> None:
        self.offline_calls: list[dict[str, object]] = []
        self.refresh_events: list[dict[str, object]] = []
        self.finalize_calls: list[dict[str, object]] = []
        self._eventsub_state_store = EventSubStateStore(
            repository=InMemoryEventSubStateRepository(),
        )

    def _load_live_state_row(self, login_lower: str) -> dict:
        return {
            "is_live": 1,
            "last_game": "Deadlock",
            "had_deadlock_in_session": 1,
            "last_started_at": "2026-03-08T12:00:00+00:00",
            "login_lower": login_lower,
        }

    def _get_tracked_logins_for_eventsub(self) -> list[str]:
        return []

    async def _fetch_streams_by_logins_quick(self, tracked_logins: list[str]) -> dict:
        del tracked_logins
        return {}

    async def _handle_auto_raid_on_offline(self, **kwargs) -> None:
        self.offline_calls.append(dict(kwargs))

    async def _request_partner_raid_score_refresh(self, **kwargs):
        self.refresh_events.append(dict(kwargs))
        return True

    async def _finalize_eventsub_offline_session(self, **kwargs):
        self.finalize_calls.append(dict(kwargs))
        return True


class _RaidArrivalHarness(_EventSubMixin):
    def __init__(self) -> None:
        self.arrivals: list[dict[str, object]] = []
        self._eventsub_state_store = EventSubStateStore(
            repository=InMemoryEventSubStateRepository(),
        )
        self._raid_bot = SimpleNamespace(on_raid_arrival=self._on_raid_arrival)

    async def _on_raid_arrival(self, **kwargs) -> None:
        self.arrivals.append(dict(kwargs))


class _DeadLetterHarness(_EventSubMixin):
    def __init__(self) -> None:
        self._eventsub_supervisor_wakeup = asyncio.Event()
        self._eventsub_retry_reason = None


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


class _GoLiveHarness(TwitchMonitoringMixin):
    def __init__(self) -> None:
        self.spawned: list[str] = []
        self.bot = SimpleNamespace(get_channel=lambda _channel_id: None)
        self._category_id = None
        self._category_sample_limit = 0
        self._tick_count = 0
        self._log_every_n = 1
        self._twitch_chat_bot = None

    def _spawn_bg_task(self, coro, name: str):
        self.spawned.append(name)
        coro.close()
        return SimpleNamespace(done=lambda: True)

    async def _ensure_stream_session(self, **_kwargs):
        return None


class _StopProcessing(Exception):
    pass


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
        self.finalize_calls: list[dict[str, object]] = []

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

    async def _finalize_stream_session(self, **kwargs):
        self.finalize_calls.append(dict(kwargs))
        return True


class _BillingHarness(_DashboardBillingMixin):
    pass


class PartnerRaidScoreRefreshTriggerTests(unittest.IsolatedAsyncioTestCase):
    async def test_stream_online_updates_live_state_and_requests_refresh(self) -> None:
        harness = _AnalyticsHarness()
        conn = _RecordingConnection()

        with patch(
            "bot.analytics.mixin.storage.transaction",
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
            "bot.analytics.mixin.storage.transaction",
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

    async def test_channel_update_prefers_scheduled_refresh_when_bg_spawn_available(self) -> None:
        harness = _AnalyticsScheduledHarness()
        conn = _RecordingConnection()

        with patch(
            "bot.analytics.mixin.storage.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            await harness._handle_channel_update(
                "3333",
                {
                    "title": "Fresh title",
                    "category_name": "Deadlock",
                },
            )

        self.assertEqual(
            harness.scheduled_refreshes,
            [
                {
                    "twitch_user_id": "3333",
                    "trigger": "eventsub_channel_update",
                }
            ],
        )

    async def test_stream_online_defers_followups_into_eventsub_inbox(self) -> None:
        harness = _AnalyticsDeferredFollowupHarness()
        conn = _RecordingConnection()

        with patch(
            "bot.analytics.mixin.storage.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            await harness._handle_stream_online(
                "4444",
                "partner_four",
                {"id": "stream-4", "started_at": "2026-03-08T12:00:00+00:00"},
            )

        self.assertEqual(harness.live_events, [])
        self.assertEqual(harness.refresh_events, [])
        self.assertEqual(
            harness.enqueued_followups,
            [
                {
                    "broadcaster_user_id": "4444",
                    "broadcaster_login": "partner_four",
                    "login_value": "partner_four",
                    "message_id": None,
                }
            ],
        )

    async def test_stream_online_business_effects_are_idempotent_per_message_id(self) -> None:
        harness = _IdempotentAnalyticsHarness()
        conn = _RecordingConnection()

        with patch(
            "bot.analytics.mixin.storage.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            await harness._handle_stream_online(
                "5555",
                "partner_five",
                {"id": "stream-5", "started_at": "2026-03-08T12:00:00+00:00"},
                message_id="msg-stream-online-once-1",
            )
            await harness._handle_stream_online(
                "5555",
                "partner_five",
                {"id": "stream-5", "started_at": "2026-03-08T12:00:00+00:00"},
                message_id="msg-stream-online-once-1",
            )

        self.assertEqual(harness.live_events, [("5555", "partner_five")])
        self.assertEqual(
            harness.refresh_events,
            [
                {
                    "twitch_user_id": "5555",
                    "login": "partner_five",
                    "trigger": "eventsub_stream_online",
                }
            ],
        )
        self.assertEqual(
            sum(1 for sql, _ in conn.executed if "INSERT INTO twitch_live_state" in sql),
            2,
        )

    async def test_channel_update_business_effects_are_idempotent_per_message_id(self) -> None:
        harness = _IdempotentAnalyticsHarness()
        conn = _RecordingConnection()

        with patch(
            "bot.analytics.mixin.storage.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            await harness._handle_channel_update(
                "6666",
                {
                    "title": "Fresh title",
                    "category_name": "Deadlock",
                    "broadcaster_language": "de",
                },
                message_id="msg-channel-update-once-1",
            )
            await harness._handle_channel_update(
                "6666",
                {
                    "title": "Fresh title",
                    "category_name": "Deadlock",
                    "broadcaster_language": "de",
                },
                message_id="msg-channel-update-once-1",
            )

        self.assertEqual(
            harness.refresh_events,
            [
                {
                    "twitch_user_id": "6666",
                    "trigger": "eventsub_channel_update",
                }
            ],
        )
        self.assertEqual(len(conn.executed), 2)
        self.assertIn("INSERT INTO twitch_channel_updates", conn.executed[0][0])
        self.assertIn("UPDATE twitch_live_state", conn.executed[1][0])

    async def test_stream_offline_business_effects_are_idempotent_per_message_id(self) -> None:
        harness = _IdempotentOfflineHarness()
        conn = _RecordingConnection()

        with patch(
            "bot.monitoring.eventsub_mixin.storage.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            await harness._on_eventsub_stream_offline(
                "7777",
                "partner_seven",
                message_id="msg-stream-offline-once-1",
                allow_scheduled_refresh=False,
            )
            harness._eventsub_offline_throttle = {}
            await harness._on_eventsub_stream_offline(
                "7777",
                "partner_seven",
                message_id="msg-stream-offline-once-1",
                allow_scheduled_refresh=False,
            )

        self.assertEqual(len(harness.finalize_calls), 1)
        self.assertEqual(
            harness.refresh_events,
            [
                {
                    "twitch_user_id": "7777",
                    "login": "partner_seven",
                    "trigger": "eventsub_stream_offline",
                }
            ],
        )
        self.assertEqual(len(harness.offline_calls), 1)
        self.assertEqual(
            sum(1 for sql, _ in conn.executed if "UPDATE twitch_live_state" in sql),
            1,
        )

    async def test_channel_raid_arrival_is_idempotent_per_message_id(self) -> None:
        harness = _RaidArrivalHarness()
        payload = {
            "message_id": "msg-channel-raid-once-1",
            "to_broadcaster_id": "8888",
            "to_broadcaster_login": "partner_eight",
            "event": {
                "from_broadcaster_user_id": "9999",
                "from_broadcaster_user_login": "raider_one",
                "viewers": 42,
            },
        }

        await harness._process_eventsub_processing_record("channel.raid", dict(payload))
        await harness._process_eventsub_processing_record("channel.raid", dict(payload))

        self.assertEqual(
            harness.arrivals,
            [
                {
                    "to_broadcaster_id": "8888",
                    "to_broadcaster_login": "partner_eight",
                    "from_broadcaster_login": "raider_one",
                    "from_broadcaster_id": "9999",
                    "viewer_count": 42,
                }
            ],
        )

    async def test_business_effect_guard_releases_claim_after_failure(self) -> None:
        harness = _IdempotentOfflineHarness()
        attempts: list[str] = []

        async def _fail_then_succeed() -> None:
            attempts.append("run")
            if len(attempts) == 1:
                raise RuntimeError("transient business failure")

        with self.assertRaisesRegex(RuntimeError, "transient business failure"):
            await harness._run_eventsub_business_effect_once(
                message_id="msg-business-effect-retry-1",
                effect_name="stream_online_went_live",
                coro_factory=_fail_then_succeed,
            )

        executed = await harness._run_eventsub_business_effect_once(
            message_id="msg-business-effect-retry-1",
            effect_name="stream_online_went_live",
            coro_factory=_fail_then_succeed,
        )

        self.assertTrue(executed)
        self.assertEqual(attempts, ["run", "run"])

    async def test_eventsub_processing_dead_letter_logs_and_wakes_supervisor(self) -> None:
        harness = _DeadLetterHarness()

        with patch("bot.monitoring.eventsub_mixin.log.critical") as log_critical:
            await harness._handle_eventsub_processing_dead_letter(
                {
                    "work_type": "channel.raid",
                    "work_id": "dead-1",
                    "message_id": "msg-dead-1",
                    "attempt_count": 5,
                    "last_error": "persistent failure",
                }
            )

        self.assertEqual(harness._eventsub_retry_reason, "processing_dead_letter")
        self.assertTrue(harness._eventsub_supervisor_wakeup.is_set())
        log_critical.assert_called_once()

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
            "bot.monitoring.eventsub_mixin.storage.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            await harness._on_eventsub_stream_offline("1234", "partner_one")

        self.assertEqual(len(harness.offline_calls), 1)
        self.assertEqual(
            harness.finalize_calls,
            [{"login": "partner_one", "reason": "offline"}],
        )
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

    async def test_process_postings_uses_managed_spawn_for_go_live_handler(self) -> None:
        harness = _GoLiveHarness()
        tracked = [
            {
                "login": "partner_one",
                "twitch_user_id": "1234",
                "require_link": False,
                "is_verified": True,
                "is_archived": False,
            }
        ]
        streams_by_login = {
            "partner_one": {
                "id": "stream-1",
                "user_login": "partner_one",
                "game_name": "Deadlock",
            }
        }
        conn = _RecordingConnection(
            fetchall_rows=[
                {
                    "streamer_login": "partner_one",
                    "twitch_user_id": "1234",
                    "is_live": 0,
                    "last_game": None,
                    "had_deadlock_in_session": 0,
                    "partner_raid_bot_enabled": 1,
                }
            ]
        )
        handler = AsyncMock()
        harness._handle_stream_went_live = handler
        harness._persist_live_state_rows = AsyncMock(side_effect=_StopProcessing)

        with (
            patch("bot.monitoring.monitoring.storage.readonly_connection", side_effect=lambda: contextlib.nullcontext(conn)),
            patch("bot.monitoring.monitoring.storage.transaction", side_effect=lambda: contextlib.nullcontext(conn)),
            patch(
                "bot.monitoring.monitoring.asyncio.create_task",
                side_effect=AssertionError("raw create_task should not be used"),
            ),
        ):
            with self.assertRaises(_StopProcessing):
                await harness._process_postings(tracked, streams_by_login)

        self.assertEqual(harness.spawned, ["golive.partner_one"])
        handler.assert_not_awaited()
        self.assertEqual(len(conn.executed), 1)
        self.assertIn("LEFT JOIN LATERAL", conn.executed[0][0])

    async def test_process_postings_falls_back_to_partner_lookup_when_live_state_row_is_missing(self) -> None:
        harness = _GoLiveHarness()
        tracked = [
            {
                "login": "partner_one",
                "twitch_user_id": "1234",
                "require_link": False,
                "is_verified": True,
                "is_archived": False,
            }
        ]
        streams_by_login = {
            "partner_one": {
                "id": "stream-1",
                "user_login": "partner_one",
                "game_name": "Deadlock",
            }
        }
        conn = _RecordingConnection(
            fetchall_rows_by_sql={
                "FROM twitch_live_state ls": [],
                "FROM twitch_partners p": [
                    {
                        "twitch_user_id": "1234",
                        "partner_raid_bot_enabled": 1,
                    }
                ],
            }
        )
        handler = AsyncMock()
        harness._handle_stream_went_live = handler
        harness._persist_live_state_rows = AsyncMock(side_effect=_StopProcessing)

        with (
            patch("bot.monitoring.monitoring.storage.readonly_connection", side_effect=lambda: contextlib.nullcontext(conn)),
            patch("bot.monitoring.monitoring.storage.transaction", side_effect=lambda: contextlib.nullcontext(conn)),
            patch(
                "bot.monitoring.monitoring.asyncio.create_task",
                side_effect=AssertionError("raw create_task should not be used"),
            ),
        ):
            with self.assertRaises(_StopProcessing):
                await harness._process_postings(tracked, streams_by_login)

        self.assertEqual(harness.spawned, ["golive.partner_one"])
        handler.assert_not_awaited()
        self.assertEqual(len(conn.executed), 2)
        self.assertIn("FROM twitch_live_state ls", conn.executed[0][0])
        self.assertIn("SELECT DISTINCT ON (p.twitch_user_id)", conn.executed[1][0])


if __name__ == "__main__":
    unittest.main()
