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
    def set_callback(self, *_args, **_kwargs) -> None:
        return None


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


if __name__ == "__main__":
    unittest.main()
