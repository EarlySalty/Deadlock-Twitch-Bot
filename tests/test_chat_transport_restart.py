from __future__ import annotations

import asyncio
import contextlib
import sqlite3
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from bot.chat import bot as chat_bot_module
from bot.chat.connection import ConnectionMixin
from tests.sqlite_twitch_schema import ensure_sqlite_twitch_schema


class _PartChannelsHarness(ConnectionMixin):
    def __init__(self) -> None:
        self._initial_channels = ["partner_channel", "cemo_336", "dragskope"]
        self._monitored_streamers = {"partner_channel", "cemo_336", "dragskope"}
        self._channel_subscription_types = {}
        self._channel_subscription_state = {}
        self._channel_ids = {
            "partner_channel": "1001",
            "cemo_336": "494921554",
            "dragskope": "128660506",
        }
        self._monitored_only_channels = {"cemo_336", "dragskope"}

    async def join(self, channel_login: str, channel_id: str | None = None):
        normalized = str(channel_login or "").strip().lower().lstrip("#")
        self._monitored_streamers.add(normalized)
        return True


class _StaleJoinHarness(ConnectionMixin):
    def __init__(self) -> None:
        self._client_id = "client-id"
        self._bot_token = "oauth:test-token"
        self._bot_refresh_token = "refresh-token"
        self._token_manager = SimpleNamespace(scopes={"user:read:chat"})
        self.bot_id_safe = "9999"
        self.bot_id = "9999"
        self._monitored_streamers = {"cemo_336"}
        self._channel_subscription_types = {"cemo_336": {"channel.chat.message"}}
        self._channel_subscription_state = {}
        self._channel_ids = {"cemo_336": "494921554"}
        self._mod_retry_cooldown = {}
        self._monitored_only_channels = set()
        self._initial_channels = ["cemo_336"]
        self._ensure_bot_is_mod = AsyncMock(return_value=False)

    async def fetch_user(self, login: str):
        return SimpleNamespace(id="494921554")

    async def _ensure_bot_token_registered(self) -> None:
        return None

    async def subscribe_websocket(self, payload) -> None:
        raise Exception("403 subscription missing proper authorization")

    def _is_monitored_only(self, channel_name: str) -> bool:
        return False


class ChatTransportRestartTests(unittest.IsolatedAsyncioTestCase):
    async def test_part_channels_prunes_initial_channel_cache(self) -> None:
        harness = _PartChannelsHarness()

        removed = await harness.part_channels(["cemo_336"])

        self.assertEqual(removed, 0)
        self.assertEqual(harness._initial_channels, ["partner_channel", "dragskope"])
        self.assertNotIn("cemo_336", harness._monitored_only_channels)
        self.assertNotIn("cemo_336", harness._monitored_streamers)

    async def test_join_channels_can_skip_monitored_only_marking(self) -> None:
        harness = _PartChannelsHarness()
        harness._monitored_only_channels.clear()

        joined = await harness.join_channels(
            ["partner_channel", "derechtecoolys"],
            rate_limit_delay=0,
            mark_monitored_only=False,
        )

        self.assertEqual(joined, 2)
        self.assertEqual(harness._monitored_only_channels, set())

    @unittest.skipUnless(
        getattr(chat_bot_module, "TWITCHIO_AVAILABLE", False)
        and hasattr(chat_bot_module, "RaidChatBot"),
        "TwitchIO chat bot unavailable in test environment",
    )
    async def test_event_ready_skips_initial_join_once_after_managed_restart(self) -> None:
        dummy = SimpleNamespace(
            user=SimpleNamespace(name="deutschedeadlockcommunity"),
            commands={},
            _skip_initial_join_once=True,
            _initial_channels=["cemo_336", "dragskope"],
            join=AsyncMock(return_value=True),
            _promo_task=None,
            _next_chat_observability_flow_id=lambda prefix: f"{prefix}-test",
            _log_chat_runtime_snapshot=lambda **kwargs: None,
        )

        with patch.object(chat_bot_module, "PROMO_MESSAGES", []):
            await chat_bot_module.RaidChatBot.event_ready(dummy)

        dummy.join.assert_not_awaited()
        self.assertFalse(dummy._skip_initial_join_once)

    @unittest.skipUnless(
        getattr(chat_bot_module, "TWITCHIO_AVAILABLE", False)
        and hasattr(chat_bot_module, "RaidChatBot"),
        "TwitchIO chat bot unavailable in test environment",
    )
    async def test_restart_after_transport_failure_clears_subscription_caches(self) -> None:
        created_coroutines = []

        def _fake_create_task(coro, *, name=None):
            created_coroutines.append((coro, name))
            if asyncio.iscoroutine(coro):
                coro.close()
            return SimpleNamespace(name=name)

        dummy = SimpleNamespace(
            _restart_lock=asyncio.Lock(),
            _managed_start_with_adapter=False,
            _managed_load_tokens=False,
            _managed_save_tokens=False,
            _skip_initial_join_once=False,
            _monitored_streamers={"partner_channel"},
            _channel_subscription_types={"partner_channel": {"channel.chat.message"}},
            _channel_subscription_state={
                "partner_channel": {"channel.chat.message": {"state": "ok"}}
            },
            _channel_ids={"partner_channel": "1001"},
            close=AsyncMock(),
            start=AsyncMock(),
            _rejoin_channels_after_restart=AsyncMock(),
            adapter=None,
        )
        dummy._bounded_runtime_sample = lambda values, limit=8: chat_bot_module.RaidChatBot._bounded_runtime_sample(  # type: ignore[assignment]
            values,
            limit=limit,
        )
        dummy._snapshot_chat_runtime_state = lambda: chat_bot_module.RaidChatBot._snapshot_chat_runtime_state(dummy)  # type: ignore[assignment]
        dummy._chat_observability_normalize = lambda value, limit=240: ConnectionMixin._chat_observability_normalize(  # type: ignore[assignment]
            value,
            limit=limit,
        )
        dummy._format_chat_observability_fields = lambda **fields: ConnectionMixin._format_chat_observability_fields(dummy, **fields)  # type: ignore[assignment]
        dummy._log_chat_runtime_snapshot = lambda **kwargs: chat_bot_module.RaidChatBot._log_chat_runtime_snapshot(dummy, **kwargs)  # type: ignore[assignment]

        with self.assertLogs("TwitchStreams.ChatBot", level="INFO") as captured:
            with patch("bot.chat.bot.asyncio.sleep", new=AsyncMock(return_value=None)):
                with patch(
                    "bot.chat.bot.asyncio.create_task",
                    side_effect=_fake_create_task,
                ):
                    await chat_bot_module.RaidChatBot._restart_after_transport_failure(
                        dummy,
                        channel_list=["partner_channel"],
                        reason="broken transport",
                        flow_id="restart-test-1",
                        failed_channel="partner_channel",
                    )

        self.assertEqual(dummy._monitored_streamers, set())
        self.assertEqual(dummy._channel_subscription_types, {})
        self.assertEqual(dummy._channel_subscription_state, {})
        self.assertEqual(dummy._channel_ids, {})
        self.assertTrue(dummy._skip_initial_join_once)
        self.assertEqual(len(created_coroutines), 2)
        self.assertTrue(
            any(
                "chat_runtime_snapshot" in entry
                and "flow_id=restart-test-1" in entry
                and "phase=restart_begin" in entry
                for entry in captured.output
            )
        )

    async def test_join_purges_stale_removed_channel_before_mod_retry(self) -> None:
        conn = sqlite3.connect(":memory:")
        conn.row_factory = sqlite3.Row
        ensure_sqlite_twitch_schema(conn)
        harness = _StaleJoinHarness()
        try:
            with patch(
                "bot.chat.connection.get_conn",
                side_effect=lambda: contextlib.nullcontext(conn),
            ):
                result = await harness.join("cemo_336", channel_id="494921554")
        finally:
            conn.close()

        self.assertFalse(result)
        harness._ensure_bot_is_mod.assert_not_awaited()
        self.assertNotIn("cemo_336", harness._initial_channels)
        self.assertNotIn("cemo_336", harness._monitored_streamers)
        state = harness.get_channel_subscription_state("cemo_336")
        self.assertEqual(state, {})


if __name__ == "__main__":
    unittest.main()
