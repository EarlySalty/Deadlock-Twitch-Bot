from __future__ import annotations

import sqlite3
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

import aiohttp

from bot.monitoring.eventsub_mixin import _EventSubMixin


class _ConnCtx:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def __enter__(self) -> sqlite3.Connection:
        return self

    def __exit__(self, exc_type, exc, tb) -> bool:
        return False

    def execute(self, sql: str, params=()):
        return self._conn.execute(str(sql or "").replace("%s", "?"), params)

    def __getattr__(self, name: str):
        return getattr(self._conn, name)


class _WebhookHandler:
    def __init__(self) -> None:
        self.dispatch_active = False
        self.revocation_callback = None

    def set_callback(self, *_args, **_kwargs) -> None:
        return None

    def set_revocation_callback(self, callback) -> None:
        self.revocation_callback = callback

    def activate_notification_dispatch(self) -> None:
        self.dispatch_active = True

    def deactivate_notification_dispatch(self) -> None:
        self.dispatch_active = False


class _DummyEventSubHarness(_EventSubMixin):
    def __init__(self) -> None:
        self.api = SimpleNamespace(
            list_eventsub_subscriptions=AsyncMock(return_value=[]),
            delete_eventsub_subscription=AsyncMock(),
            subscribe_eventsub_webhook=AsyncMock(),
        )
        self.bot = SimpleNamespace(wait_until_ready=AsyncMock())
        self._webhook_secret = "secret"
        self._eventsub_webhook_handler = _WebhookHandler()
        self._twitch_chat_bot = None
        self._eventsub_started = False
        self._eventsub_webhook_active_subs = []

    def _get_eventsub_webhook_url(self) -> str:
        return "https://example.com/twitch/eventsub/callback"

    def _get_raid_enabled_streamers_for_eventsub(self) -> list[dict[str, str]]:
        return []

    async def _record_eventsub_capacity_snapshot(
        self,
        reason: str,
        *,
        force: bool = False,
    ) -> None:
        del reason, force
        return None


class EventSubModeratorSubscriptionFallbackTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self.conn = sqlite3.connect(":memory:")
        self.conn.row_factory = sqlite3.Row
        self.conn.execute(
            """
            CREATE TABLE twitch_raid_auth (
                twitch_user_id TEXT,
                scopes TEXT
            )
            """
        )
        self.conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, scopes)
            VALUES (?, ?)
            """,
            ("123", "moderator:read:followers"),
        )
        self.conn.commit()

    def tearDown(self) -> None:
        self.conn.close()

    async def test_follow_subscription_falls_back_to_broadcaster_when_bot_attempt_fails(self) -> None:
        harness = _DummyEventSubHarness()
        harness._resolve_eventsub_bot_auth = AsyncMock(
            return_value=("bot-token", "999", {"moderator:read:followers"})
        )
        harness._resolve_eventsub_broadcaster_token = AsyncMock(
            return_value="broadcaster-token"
        )
        harness._is_fully_authed = AsyncMock(return_value=True)

        def _client_error(status: int, message: str) -> aiohttp.ClientResponseError:
            return aiohttp.ClientResponseError(
                request_info=SimpleNamespace(real_url="https://example.com"),
                history=(),
                status=status,
                message=message,
            )

        async def _subscribe_side_effect(**kwargs):
            if (
                kwargs.get("sub_type") == "channel.follow"
                and kwargs.get("oauth_token") == "bot-token"
            ):
                raise _client_error(403, "bot not moderator")
            return {"data": [{"id": "sub-1"}]}

        harness.api.subscribe_eventsub_webhook.side_effect = _subscribe_side_effect

        with patch(
            "bot.monitoring.eventsub_mixin.storage.readonly_connection",
            return_value=_ConnCtx(self.conn),
        ):
            await harness._start_eventsub_listener()
            await harness._handle_stream_went_live("123", "partner_one")

        follow_calls = [
            call.kwargs
            for call in harness.api.subscribe_eventsub_webhook.await_args_list
            if call.kwargs.get("sub_type") == "channel.follow"
        ]

        self.assertEqual(len(follow_calls), 2)
        self.assertEqual(follow_calls[0]["oauth_token"], "bot-token")
        self.assertEqual(
            follow_calls[0]["condition"],
            {"broadcaster_user_id": "123", "moderator_user_id": "999"},
        )
        self.assertEqual(follow_calls[1]["oauth_token"], "broadcaster-token")
        self.assertEqual(
            follow_calls[1]["condition"],
            {"broadcaster_user_id": "123", "moderator_user_id": "123"},
        )

    async def test_stream_offline_duplicate_does_not_abort_remaining_go_live_followups(self) -> None:
        harness = _DummyEventSubHarness()
        harness._resolve_eventsub_bot_auth = AsyncMock(
            return_value=("bot-token", "999", {"moderator:read:followers"})
        )
        harness._resolve_eventsub_broadcaster_token = AsyncMock(return_value=None)
        harness._is_fully_authed = AsyncMock(return_value=True)

        def _client_error(status: int, message: str) -> aiohttp.ClientResponseError:
            return aiohttp.ClientResponseError(
                request_info=SimpleNamespace(real_url="https://example.com"),
                history=(),
                status=status,
                message=message,
            )

        async def _subscribe_side_effect(**kwargs):
            if kwargs.get("sub_type") == "stream.offline":
                raise _client_error(409, "subscription already exists")
            return {"data": [{"id": "sub-1"}]}

        harness.api.subscribe_eventsub_webhook.side_effect = _subscribe_side_effect

        with patch(
            "bot.monitoring.eventsub_mixin.storage.readonly_connection",
            return_value=_ConnCtx(self.conn),
        ):
            await harness._start_eventsub_listener()
            await harness._handle_stream_went_live("123", "partner_one")

        follow_calls = [
            call.kwargs
            for call in harness.api.subscribe_eventsub_webhook.await_args_list
            if call.kwargs.get("sub_type") == "channel.follow"
        ]

        self.assertEqual(len(follow_calls), 1)
        self.assertEqual(follow_calls[0]["oauth_token"], "bot-token")
        self.assertIn(("stream.offline", "123"), harness._eventsub_webhook_tracked)

    async def test_wait_until_ready_failure_does_not_install_golive_handler(self) -> None:
        harness = _DummyEventSubHarness()
        harness.bot.wait_until_ready = AsyncMock(side_effect=RuntimeError("discord not ready"))

        await harness._start_eventsub_listener()

        self.assertFalse(hasattr(harness, "_handle_stream_went_live"))
        harness.api.subscribe_eventsub_webhook.assert_not_awaited()

    async def test_webhook_startup_retries_when_core_subscriptions_are_largely_missing(self) -> None:
        class _SingleStreamerHarness(_DummyEventSubHarness):
            def _get_raid_enabled_streamers_for_eventsub(self) -> list[dict[str, str]]:
                return [{"twitch_user_id": "123", "twitch_login": "partner_one"}]

        harness = _SingleStreamerHarness()

        async def _subscribe_side_effect(**kwargs):
            if kwargs.get("sub_type") == "stream.online":
                return {"data": [{"id": "sub-online"}]}
            raise RuntimeError("twitch outage")

        harness.api.subscribe_eventsub_webhook.side_effect = _subscribe_side_effect

        started = await harness._start_eventsub_listener()

        self.assertFalse(started)
        self.assertFalse(harness._eventsub_started)
        self.assertEqual(harness._eventsub_retry_reason, "webhook_startup_incomplete")
        self.assertFalse(harness._eventsub_webhook_handler.dispatch_active)

    async def test_webhook_startup_requires_stream_online_and_offline_per_broadcaster(self) -> None:
        class _SingleStreamerHarness(_DummyEventSubHarness):
            def _get_raid_enabled_streamers_for_eventsub(self) -> list[dict[str, str]]:
                return [{"twitch_user_id": "123", "twitch_login": "partner_one"}]

        harness = _SingleStreamerHarness()

        async def _subscribe_side_effect(**kwargs):
            if kwargs.get("sub_type") in {"stream.online", "channel.update"}:
                return {"data": [{"id": f"sub-{kwargs.get('sub_type')}"}]}
            raise RuntimeError("stream.offline missing")

        harness.api.subscribe_eventsub_webhook.side_effect = _subscribe_side_effect

        started = await harness._start_eventsub_listener()

        self.assertFalse(started)
        self.assertEqual(harness._eventsub_retry_reason, "webhook_startup_incomplete")
        self.assertFalse(harness._eventsub_webhook_handler.dispatch_active)

    async def test_webhook_startup_health_is_evaluated_per_broadcaster(self) -> None:
        class _TwoStreamerHarness(_DummyEventSubHarness):
            def _get_raid_enabled_streamers_for_eventsub(self) -> list[dict[str, str]]:
                return [
                    {"twitch_user_id": "123", "twitch_login": "partner_one"},
                    {"twitch_user_id": "456", "twitch_login": "partner_two"},
                ]

        harness = _TwoStreamerHarness()

        async def _subscribe_side_effect(**kwargs):
            sub_type = kwargs.get("sub_type")
            broadcaster_id = str((kwargs.get("condition") or {}).get("broadcaster_user_id") or "")
            if broadcaster_id == "123":
                return {"data": [{"id": f"sub-{sub_type}-{broadcaster_id}"}]}
            if broadcaster_id == "456" and sub_type == "channel.update":
                return {"data": [{"id": f"sub-{sub_type}-{broadcaster_id}"}]}
            raise RuntimeError("transient twitch outage")

        harness.api.subscribe_eventsub_webhook.side_effect = _subscribe_side_effect

        started = await harness._start_eventsub_listener()

        self.assertFalse(started)
        self.assertEqual(harness._eventsub_retry_reason, "webhook_startup_incomplete")
        self.assertFalse(harness._eventsub_webhook_handler.dispatch_active)

    async def test_webhook_dispatch_is_activated_after_cleanup_finishes(self) -> None:
        class _SingleStreamerHarness(_DummyEventSubHarness):
            def _get_raid_enabled_streamers_for_eventsub(self) -> list[dict[str, str]]:
                return [{"twitch_user_id": "123", "twitch_login": "partner_one"}]

        harness = _SingleStreamerHarness()
        harness.api.subscribe_eventsub_webhook.return_value = {"data": [{"id": "sub-1"}]}

        async def _cleanup_side_effect(
            _webhook_url: str,
            *,
            active_target_user_ids: set[str] | None = None,
        ) -> None:
            self.assertFalse(harness._eventsub_webhook_handler.dispatch_active)
            self.assertEqual(active_target_user_ids, {"123"})

        harness._cleanup_old_eventsub_subscriptions = AsyncMock(side_effect=_cleanup_side_effect)

        started = await harness._start_eventsub_listener()

        self.assertTrue(started)
        self.assertTrue(harness._eventsub_webhook_handler.dispatch_active)
        harness._cleanup_old_eventsub_subscriptions.assert_awaited_once_with(
            "https://example.com/twitch/eventsub/callback",
            active_target_user_ids={"123"},
        )

    async def test_webhook_startup_does_not_cleanup_remote_subscriptions_before_health_is_confirmed(
        self,
    ) -> None:
        class _SingleStreamerHarness(_DummyEventSubHarness):
            def _get_raid_enabled_streamers_for_eventsub(self) -> list[dict[str, str]]:
                return [{"twitch_user_id": "123", "twitch_login": "partner_one"}]

        harness = _SingleStreamerHarness()

        async def _subscribe_side_effect(**kwargs):
            if kwargs.get("sub_type") == "stream.online":
                return {"data": [{"id": "sub-online"}]}
            raise RuntimeError("twitch outage")

        harness.api.subscribe_eventsub_webhook.side_effect = _subscribe_side_effect
        harness._cleanup_old_eventsub_subscriptions = AsyncMock()

        started = await harness._start_eventsub_listener()

        self.assertFalse(started)
        harness._cleanup_old_eventsub_subscriptions.assert_not_awaited()

    async def test_webhook_revocation_marks_listener_unhealthy_and_wakes_supervisor(self) -> None:
        harness = _DummyEventSubHarness()
        harness._eventsub_started = True
        harness._eventsub_webhook_tracked = {("stream.offline", "123")}
        harness._eventsub_webhook_active_subs = [
            {"sub_type": "stream.offline", "broadcaster_user_id": "123"}
        ]
        wakeup_reasons: list[str] = []
        harness._ensure_eventsub_supervisor_running = wakeup_reasons.append  # type: ignore[method-assign]

        await harness._handle_eventsub_webhook_revocation(
            {
                "subscription": {
                    "type": "stream.offline",
                    "condition": {"broadcaster_user_id": "123"},
                }
            },
            message_id="msg-revocation-1",
        )

        self.assertFalse(harness._eventsub_started)
        self.assertEqual(harness._eventsub_retry_reason, "webhook_revocation")
        self.assertFalse(harness._eventsub_webhook_handler.dispatch_active)
        self.assertEqual(wakeup_reasons, ["webhook_revocation"])
        self.assertNotIn(("stream.offline", "123"), harness._eventsub_webhook_tracked)
        self.assertEqual(harness._eventsub_webhook_active_subs, [])


if __name__ == "__main__":
    unittest.main()
