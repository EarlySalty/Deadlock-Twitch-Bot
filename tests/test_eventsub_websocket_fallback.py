import asyncio
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from bot.monitoring.eventsub_mixin import _EventSubMixin
from bot.monitoring.eventsub_state_store import EventSubStateStore
from bot.monitoring.eventsub_ws import EventSubWSListener
from tests.eventsub_state_store_test_helpers import InMemoryEventSubStateRepository


class _FakeWsApi:
    def __init__(self) -> None:
        self.calls: list[dict[str, object]] = []

    async def subscribe_eventsub_websocket(
        self,
        *,
        session_id: str,
        sub_type: str,
        condition: dict,
        oauth_token: str,
    ) -> None:
        self.calls.append(
            {
                "session_id": session_id,
                "sub_type": sub_type,
                "condition": condition,
                "oauth_token": oauth_token,
            }
        )


class _FakeWSPool:
    def __init__(self, *, api, logger, token_resolver, state_store=None) -> None:
        self.api = api
        self.logger = logger
        self.token_resolver = token_resolver
        self.state_store = state_store
        self.callbacks: dict[str, object] = {}
        self.subscriptions: list[tuple[str, str, dict[str, str]]] = []
        self.run_calls = 0
        self.wait_initial_calls = 0
        self.wait_initial_result = True
        self.fail_stream_offline_add = False

    def set_callback(self, sub_type: str, callback) -> None:
        self.callbacks[sub_type] = callback

    def add_subscription(
        self,
        sub_type: str,
        broadcaster_id: str,
        condition: dict | None = None,
    ) -> bool:
        self.subscriptions.append((sub_type, str(broadcaster_id), dict(condition or {})))
        if self.fail_stream_offline_add and sub_type == "stream.offline":
            return False
        return True

    async def wait_until_initial_registration(
        self,
        timeout: float = 8.0,
        poll_interval: float = 0.1,
    ) -> bool:
        del timeout, poll_interval
        self.wait_initial_calls += 1
        return self.wait_initial_result

    async def run(self) -> None:
        self.run_calls += 1

    def stop(self) -> None:
        return None

    @property
    def subscription_count(self) -> int:
        return len(self.subscriptions)

    @property
    def listener_count(self) -> int:
        return 1


class _FakeRaidReadinessPool:
    def __init__(self, api) -> None:
        self.api = api
        self.is_ready = True
        self.wait_until_ready_calls = 0
        self.subscription_ready = False
        self.wait_result = True

    async def wait_until_ready(
        self,
        timeout: float = 8.0,
        poll_interval: float = 0.1,
    ) -> bool:
        del timeout, poll_interval
        self.wait_until_ready_calls += 1
        return self.wait_result

    def is_subscription_ready(
        self,
        sub_type: str,
        broadcaster_id: str,
        condition: dict | None = None,
    ) -> bool:
        return (
            sub_type == "channel.raid"
            and str(broadcaster_id) == "123"
            and dict(condition or {}) == {"to_broadcaster_user_id": "123"}
            and self.subscription_ready
        )

    async def add_subscription_dynamic(
        self,
        sub_type: str,
        broadcaster_id: str,
        condition: dict | None = None,
        oauth_token: str | None = None,
    ) -> bool:
        await self.api.subscribe_eventsub_websocket(
            session_id="session-1",
            sub_type=sub_type,
            condition=dict(condition or {}),
            oauth_token=str(oauth_token or "test-token"),
        )
        return True


class _WsFallbackHarness(_EventSubMixin):
    def __init__(self) -> None:
        self.api = _FakeWsApi()
        self.bot = SimpleNamespace(wait_until_ready=AsyncMock())
        self._eventsub_started = False
        self._eventsub_webhook_active_subs = []
        self._eventsub_webhook_tracked = set()
        self.snapshot_reasons: list[str] = []

    def _get_eventsub_webhook_url(self) -> str | None:
        return None

    def _get_raid_enabled_streamers_for_eventsub(self) -> list[dict[str, str]]:
        return [{"twitch_user_id": "123", "twitch_login": "partner_one"}]

    async def _resolve_eventsub_bot_token(self) -> str | None:
        return "oauth:test-token"

    async def _record_eventsub_capacity_snapshot(self, reason: str, *, force: bool = False) -> None:
        del force
        self.snapshot_reasons.append(reason)

    async def _handle_stream_online(self, broadcaster_id: str, broadcaster_login: str, event: dict):
        del broadcaster_id, broadcaster_login, event

    async def _handle_channel_update(self, broadcaster_id: str, event: dict):
        del broadcaster_id, event

    async def _on_eventsub_stream_offline(self, broadcaster_id: str, broadcaster_login: str | None):
        del broadcaster_id, broadcaster_login


class EventSubWebsocketFallbackTests(unittest.IsolatedAsyncioTestCase):
    async def test_websocket_fallback_go_live_handler_creates_dynamic_offline_subscription(
        self,
    ) -> None:
        harness = _WsFallbackHarness()
        harness._eventsub_ws_listener = _FakeRaidReadinessPool(harness.api)
        harness._install_stream_went_live_handler()

        await harness._handle_stream_went_live("123", "partner_one")

        self.assertEqual(
            harness.api.calls,
            [
                {
                    "session_id": "session-1",
                    "sub_type": "stream.offline",
                    "condition": {"broadcaster_user_id": "123"},
                    "oauth_token": "test-token",
                }
            ],
        )
        self.assertIn(("stream.offline", "123"), harness._eventsub_webhook_tracked)

    async def test_start_eventsub_listener_uses_websocket_fallback_when_webhook_is_missing(self) -> None:
        harness = _WsFallbackHarness()
        created: list[_FakeWSPool] = []

        def _listener_factory(*, api, logger, token_resolver, state_store=None):
            listener = _FakeWSPool(
                api=api,
                logger=logger,
                token_resolver=token_resolver,
                state_store=state_store,
            )
            created.append(listener)
            return listener

        with patch(
            "bot.monitoring.eventsub_mixin.EventSubWSListenerPool",
            side_effect=_listener_factory,
        ):
            await harness._start_eventsub_listener()

        self.assertEqual(len(created), 1)
        listener = created[0]
        self.assertEqual(listener.run_calls, 1)
        self.assertEqual(listener.wait_initial_calls, 1)
        self.assertCountEqual(
            [sub_type for sub_type, _, _ in listener.subscriptions],
            ["stream.online", "stream.offline", "channel.update"],
        )
        self.assertIn("stream.online", listener.callbacks)
        self.assertIn("stream.offline", listener.callbacks)
        self.assertIn("channel.raid", listener.callbacks)
        self.assertIn("channel.update", listener.callbacks)
        self.assertIn("startup_distribution", harness.snapshot_reasons)
        self.assertEqual(harness._eventsub_webhook_tracked, set())
        self.assertTrue(callable(getattr(harness, "_handle_stream_went_live", None)))

    async def test_websocket_fallback_installs_go_live_handler_that_resets_offline_throttle(self) -> None:
        harness = _WsFallbackHarness()
        harness._eventsub_offline_throttle = {"123": 100.0}

        with patch(
            "bot.monitoring.eventsub_mixin.EventSubWSListenerPool",
            side_effect=lambda **kwargs: _FakeWSPool(**kwargs),
        ):
            await harness._start_eventsub_listener()

        await harness._handle_stream_went_live("123", "partner_one")

        self.assertEqual(harness._eventsub_offline_throttle, {})

    async def test_websocket_fallback_core_callbacks_enqueue_processing_work(self) -> None:
        harness = _WsFallbackHarness()
        harness._enqueue_eventsub_stream_online_processing = AsyncMock(return_value=None)  # type: ignore[method-assign]
        harness._enqueue_eventsub_channel_update_processing = AsyncMock(return_value=None)  # type: ignore[method-assign]
        harness._enqueue_eventsub_raid_processing = AsyncMock(return_value=None)  # type: ignore[method-assign]
        created: list[_FakeWSPool] = []

        def _listener_factory(*, api, logger, token_resolver, state_store=None):
            listener = _FakeWSPool(
                api=api,
                logger=logger,
                token_resolver=token_resolver,
                state_store=state_store,
            )
            created.append(listener)
            return listener

        with patch(
            "bot.monitoring.eventsub_mixin.EventSubWSListenerPool",
            side_effect=_listener_factory,
        ):
            await harness._start_eventsub_listener()

        listener = created[0]
        await listener.callbacks["stream.online"](
            "123",
            "partner_one",
            {"broadcaster_user_id": "123", "broadcaster_user_login": "partner_one"},
            message_id="msg-online-1",
        )
        await listener.callbacks["channel.update"](
            "123",
            "partner_one",
            {"broadcaster_user_id": "123", "title": "New Title"},
            message_id="msg-update-1",
        )
        await listener.callbacks["channel.raid"](
            "123",
            "partner_one",
            {
                "to_broadcaster_user_id": "123",
                "to_broadcaster_user_login": "partner_one",
                "from_broadcaster_user_id": "999",
                "from_broadcaster_user_login": "raider",
                "viewers": 42,
            },
            message_id="msg-raid-1",
        )

        harness._enqueue_eventsub_stream_online_processing.assert_awaited_once_with(
            "123",
            "partner_one",
            {"broadcaster_user_id": "123", "broadcaster_user_login": "partner_one"},
            message_id="msg-online-1",
        )
        harness._enqueue_eventsub_channel_update_processing.assert_awaited_once_with(
            "123",
            "partner_one",
            {"broadcaster_user_id": "123", "title": "New Title"},
            message_id="msg-update-1",
        )
        harness._enqueue_eventsub_raid_processing.assert_awaited_once_with(
            "123",
            "partner_one",
            {
                "to_broadcaster_user_id": "123",
                "to_broadcaster_user_login": "partner_one",
                "from_broadcaster_user_id": "999",
                "from_broadcaster_user_login": "raider",
                "viewers": 42,
            },
            message_id="msg-raid-1",
        )

    async def test_websocket_fallback_uses_enqueue_callbacks_instead_of_inline_core_processing(
        self,
    ) -> None:
        harness = _WsFallbackHarness()
        harness._enqueue_eventsub_stream_online_processing = AsyncMock(return_value=None)  # type: ignore[method-assign]
        harness._enqueue_eventsub_raid_processing = AsyncMock(return_value=None)  # type: ignore[method-assign]
        harness._handle_stream_online = AsyncMock(  # type: ignore[method-assign]
            side_effect=AssertionError("stream.online must not run inline on WS fallback")
        )
        harness._raid_bot = SimpleNamespace(
            on_raid_arrival=AsyncMock(
                side_effect=AssertionError("channel.raid must not run inline on WS fallback")
            )
        )
        created: list[_FakeWSPool] = []

        def _listener_factory(*, api, logger, token_resolver, state_store=None):
            listener = _FakeWSPool(
                api=api,
                logger=logger,
                token_resolver=token_resolver,
                state_store=state_store,
            )
            created.append(listener)
            return listener

        with patch(
            "bot.monitoring.eventsub_mixin.EventSubWSListenerPool",
            side_effect=_listener_factory,
        ):
            await harness._start_eventsub_listener()

        listener = created[0]
        await listener.callbacks["stream.online"](
            "123",
            "partner_one",
            {"broadcaster_user_id": "123", "broadcaster_user_login": "partner_one"},
            message_id="msg-online-ws-enqueue-1",
        )
        await listener.callbacks["channel.raid"](
            "123",
            "partner_one",
            {
                "to_broadcaster_user_id": "123",
                "to_broadcaster_user_login": "partner_one",
                "from_broadcaster_user_id": "999",
                "from_broadcaster_user_login": "raider",
                "viewers": 42,
            },
            message_id="msg-raid-ws-enqueue-1",
        )

        harness._enqueue_eventsub_stream_online_processing.assert_awaited_once_with(
            "123",
            "partner_one",
            {"broadcaster_user_id": "123", "broadcaster_user_login": "partner_one"},
            message_id="msg-online-ws-enqueue-1",
        )
        harness._enqueue_eventsub_raid_processing.assert_awaited_once_with(
            "123",
            "partner_one",
            {
                "to_broadcaster_user_id": "123",
                "to_broadcaster_user_login": "partner_one",
                "from_broadcaster_user_id": "999",
                "from_broadcaster_user_login": "raider",
                "viewers": 42,
            },
            message_id="msg-raid-ws-enqueue-1",
        )
        harness._handle_stream_online.assert_not_awaited()
        harness._raid_bot.on_raid_arrival.assert_not_awaited()

    async def test_startup_with_no_streamers_resets_started_flag(self) -> None:
        class _NoStreamerHarness(_WsFallbackHarness):
            def _get_raid_enabled_streamers_for_eventsub(self) -> list[dict[str, str]]:
                return []

        harness = _NoStreamerHarness()

        with patch(
            "bot.monitoring.eventsub_mixin.EventSubWSListenerPool",
            side_effect=lambda **kwargs: _FakeWSPool(**kwargs),
        ):
            await harness._start_eventsub_listener()

        self.assertFalse(harness._eventsub_started)
        self.assertIn("startup_no_streamers", harness.snapshot_reasons)

    async def test_websocket_fallback_fails_closed_when_capacity_is_exhausted(self) -> None:
        class _ManyStreamerHarness(_WsFallbackHarness):
            def _get_raid_enabled_streamers_for_eventsub(self) -> list[dict[str, str]]:
                return [
                    {"twitch_user_id": "123", "twitch_login": "partner_one"},
                    {"twitch_user_id": "456", "twitch_login": "partner_two"},
                ]

        harness = _ManyStreamerHarness()

        def _listener_factory(*, api, logger, token_resolver, state_store=None):
            listener = _FakeWSPool(
                api=api,
                logger=logger,
                token_resolver=token_resolver,
                state_store=state_store,
            )
            listener.fail_stream_offline_add = True
            return listener

        with patch(
            "bot.monitoring.eventsub_mixin.EventSubWSListenerPool",
            side_effect=_listener_factory,
        ):
            started = await harness._start_eventsub_listener()

        self.assertFalse(started)
        self.assertEqual(harness._eventsub_retry_reason, "ws_capacity_exhausted")
        self.assertFalse(harness._eventsub_started)
        self.assertIn("startup_capacity_exhausted", harness.snapshot_reasons)

    async def test_websocket_fallback_retries_when_initial_registration_times_out(self) -> None:
        harness = _WsFallbackHarness()

        def _listener_factory(*, api, logger, token_resolver, state_store=None):
            listener = _FakeWSPool(
                api=api,
                logger=logger,
                token_resolver=token_resolver,
                state_store=state_store,
            )
            listener.wait_initial_result = False
            return listener

        with patch(
            "bot.monitoring.eventsub_mixin.EventSubWSListenerPool",
            side_effect=_listener_factory,
        ):
            started = await harness._start_eventsub_listener()

        self.assertFalse(started)
        self.assertEqual(harness._eventsub_retry_reason, "ws_initial_registration_timeout")
        self.assertFalse(harness._eventsub_started)

    async def test_raid_readiness_uses_websocket_listener_when_webhook_transport_is_unavailable(self) -> None:
        harness = _WsFallbackHarness()
        listener = EventSubWSListener(
            api=harness.api,
            token_resolver=harness._resolve_eventsub_ws_token,
        )
        listener._session_id = "session-1"
        harness._eventsub_ws_listener = listener

        ready, detail = await harness.ensure_raid_target_dynamic_ready("123", "targetlogin")

        self.assertTrue(ready)
        self.assertEqual(detail, "ws_subscribed")
        self.assertEqual(
            harness.api.calls,
            [
                {
                    "session_id": "session-1",
                    "sub_type": "channel.raid",
                    "condition": {"to_broadcaster_user_id": "123"},
                    "oauth_token": "test-token",
                }
            ],
        )
        self.assertIn(("channel.raid", "123"), harness._eventsub_webhook_tracked)

    async def test_raid_readiness_does_not_treat_pool_global_readiness_as_subscription_readiness(
        self,
    ) -> None:
        harness = _WsFallbackHarness()
        harness._eventsub_webhook_tracked.add(("channel.raid", "123"))
        pool = _FakeRaidReadinessPool(harness.api)
        harness._get_eventsub_ws_listener = lambda: pool  # type: ignore[method-assign]

        ready, detail = await harness.ensure_raid_target_dynamic_ready("123", "targetlogin")

        self.assertTrue(ready)
        self.assertEqual(detail, "ws_subscribed")
        self.assertEqual(pool.wait_until_ready_calls, 2)
        self.assertEqual(
            harness.api.calls,
            [
                {
                    "session_id": "session-1",
                    "sub_type": "channel.raid",
                    "condition": {"to_broadcaster_user_id": "123"},
                    "oauth_token": "test-token",
                }
            ],
        )

    async def test_eventsub_supervisor_retries_after_transient_start_failure(self) -> None:
        attempts: list[str] = []

        class _SupervisorHarness(_EventSubMixin):
            def __init__(self) -> None:
                self._eventsub_started = False

            async def _start_eventsub_listener(self) -> bool:
                attempts.append("attempt")
                self._eventsub_started = False
                return len(attempts) >= 2

        harness = _SupervisorHarness()
        harness._eventsub_retry_delay_seconds = lambda _failures: 0.01  # type: ignore[method-assign]

        await harness._run_eventsub_listener_supervisor()

        self.assertEqual(len(attempts), 2)

    async def test_eventsub_supervisor_idles_without_backoff_when_no_streamers_exist(self) -> None:
        attempts: list[str] = []

        class _IdleSupervisorHarness(_EventSubMixin):
            def __init__(self) -> None:
                self._eventsub_started = False

            async def _start_eventsub_listener(self) -> bool:
                attempts.append("attempt")
                self._eventsub_started = False
                if len(attempts) == 1:
                    self._eventsub_retry_reason = "no_streamers"
                    return False
                return True

        harness = _IdleSupervisorHarness()
        harness._eventsub_retry_delay_seconds = lambda _failures: (_ for _ in ()).throw(  # type: ignore[method-assign]
            AssertionError("no_streamers should not enter timed backoff")
        )

        task = asyncio.create_task(harness._run_eventsub_listener_supervisor())
        await asyncio.sleep(0)
        await asyncio.sleep(0)
        self.assertEqual(len(attempts), 1)

        harness._request_eventsub_supervisor_wakeup("partner_added")
        await task

        self.assertEqual(len(attempts), 2)

    async def test_raid_dynamic_subscription_requests_supervisor_wakeup_when_listener_missing(
        self,
    ) -> None:
        harness = _WsFallbackHarness()
        wakeup_reasons: list[str] = []
        harness._request_eventsub_supervisor_wakeup = wakeup_reasons.append  # type: ignore[method-assign]

        success = await harness.subscribe_raid_target_dynamic("123", "targetlogin")

        self.assertFalse(success)
        self.assertEqual(wakeup_reasons, ["raid_subscribe_no_listener"])
        self.assertIn("raid_subscribe_no_listener", harness.snapshot_reasons)

    async def test_raid_dynamic_subscription_requests_supervisor_wakeup_when_listener_not_ready(
        self,
    ) -> None:
        harness = _WsFallbackHarness()
        pool = _FakeRaidReadinessPool(harness.api)
        pool.wait_result = False
        harness._eventsub_ws_listener = pool
        wakeup_reasons: list[str] = []
        harness._request_eventsub_supervisor_wakeup = wakeup_reasons.append  # type: ignore[method-assign]

        success = await harness.subscribe_raid_target_dynamic("123", "targetlogin")

        self.assertFalse(success)
        self.assertEqual(wakeup_reasons, ["raid_subscribe_not_ready"])
        self.assertIn("raid_subscribe_not_ready", harness.snapshot_reasons)

    async def test_offline_throttle_is_restart_stable_via_persistent_state_store(self) -> None:
        class _OfflineThrottleHarness(_EventSubMixin):
            def __init__(self, state_store: EventSubStateStore) -> None:
                self._eventsub_state_store = state_store
                self._eventsub_enable_persistent_guards = True
                self.finalized: list[str] = []
                self.auto_raid_calls: list[str] = []

            def _resolve_eventsub_broadcaster_login(
                self,
                broadcaster_id: str,
                broadcaster_login: str | None = None,
            ) -> str:
                del broadcaster_id
                return str(broadcaster_login or "partner_one")

            def _load_live_state_row(self, login_lower: str):
                del login_lower
                return None

            async def _finalize_eventsub_offline_session(
                self,
                *,
                broadcaster_id: str,
                login_lower: str,
            ) -> None:
                del login_lower
                self.finalized.append(broadcaster_id)

            def _get_tracked_logins_for_eventsub(self) -> list[str]:
                return []

            async def _fetch_streams_by_logins_quick(self, tracked_logins: list[str]) -> dict[str, dict]:
                del tracked_logins
                return {}

            async def _handle_auto_raid_on_offline(self, **kwargs) -> None:
                self.auto_raid_calls.append(str(kwargs.get("twitch_user_id") or ""))

        state_store = EventSubStateStore(
            repository=InMemoryEventSubStateRepository(),
        )
        first = _OfflineThrottleHarness(state_store)
        second = _OfflineThrottleHarness(state_store)

        await first._on_eventsub_stream_offline("123", "partner_one")
        await second._on_eventsub_stream_offline("123", "partner_one")

        self.assertEqual(first.finalized, ["123"])
        self.assertEqual(first.auto_raid_calls, ["123"])
        self.assertEqual(second.finalized, [])
        self.assertEqual(second.auto_raid_calls, [])


if __name__ == "__main__":
    unittest.main()
