from __future__ import annotations

import sqlite3
import unittest
from contextlib import ExitStack
from unittest.mock import AsyncMock, patch

from bot.chat.promos import PromoMixin
from bot.promo_mode import ensure_global_promo_mode_storage, save_global_promo_mode


class _CompatConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    @staticmethod
    def _translate(sql: str) -> str:
        translated = str(sql)
        translated = translated.replace("%s", "?")
        translated = translated.replace(
            "NOW() - INTERVAL '30 days'",
            "datetime('now', '-30 days')",
        )
        translated = translated.replace(
            "NOW() - INTERVAL '7 days'",
            "datetime('now', '-7 days')",
        )
        return translated

    def execute(self, sql: str, params=()):
        return self._conn.execute(self._translate(sql), params)

    def __getattr__(self, name: str):
        return getattr(self._conn, name)


class _ConnCtx:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = _CompatConn(conn)

    def __enter__(self) -> _CompatConn:
        return self._conn

    def __exit__(self, exc_type, exc, tb) -> bool:
        return False


class _DummyPromoChat(PromoMixin):
    def __init__(self) -> None:
        self.announcement_calls: list[dict[str, str]] = []
        self._last_promo_sent: dict[str, float] = {}

    async def _get_promo_invite(self, login: str):
        del login
        return "https://discord.gg/example", False

    async def _send_announcement(self, channel, text: str, color: str = "purple", source: str = ""):
        self.announcement_calls.append(
            {
                "login": str(getattr(channel, "name", "") or ""),
                "channel_id": str(getattr(channel, "id", "") or ""),
                "text": text,
                "color": color,
                "source": source,
            }
        )
        return True

    async def _send_chat_message(self, channel, text: str, source: str = ""):
        self.announcement_calls.append(
            {
                "login": str(getattr(channel, "name", "") or ""),
                "channel_id": str(getattr(channel, "id", "") or ""),
                "text": text,
                "color": "",
                "source": source,
            }
        )
        return True


class ChatPromoOverrideTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self.conn = sqlite3.connect(":memory:")
        self.conn.row_factory = sqlite3.Row
        self.conn.execute(
            """
            CREATE TABLE streamer_plans (
                twitch_login TEXT PRIMARY KEY,
                promo_message TEXT
            )
            """
        )
        ensure_global_promo_mode_storage(self.conn)
        self.handler = _DummyPromoChat()

    def tearDown(self) -> None:
        self.conn.close()

    def _conn_patch(self):
        stack = ExitStack()
        stack.enter_context(
            patch("bot.chat.promos.readonly_connection", return_value=_ConnCtx(self.conn))
        )
        stack.enter_context(
            patch("bot.chat.promos.transaction", return_value=_ConnCtx(self.conn))
        )
        return stack

    async def test_active_global_event_overrides_streamer_message(self) -> None:
        self.conn.execute(
            "INSERT INTO streamer_plans (twitch_login, promo_message) VALUES (?, ?)",
            ("partner_one", "Streamer Override {invite}"),
        )
        save_global_promo_mode(
            self.conn,
            config={
                "mode": "custom_event",
                "custom_message": "Global Event {invite}",
                "is_enabled": True,
            },
            updated_by="discord:55",
        )

        with self._conn_patch(), patch(
            "bot.chat.promos.PROMO_MESSAGES",
            ["Default Fallback {invite}"],
        ), patch(
            "bot.chat.promos.PROMO_MESSAGES_CATEGORIZED",
            {},
        ), patch(
            "bot.chat.promos.PROMO_CHANNEL_ALLOWLIST",
            [],
        ):
            ok = await self.handler._send_promo_message(
                "partner_one",
                "1001",
                0.0,
                reason="chat_activity",
            )

        self.assertTrue(ok)
        self.assertEqual(self.handler.announcement_calls[0]["text"], "Global Event https://discord.gg/example")

    async def test_active_global_event_without_invite_uses_fixed_text(self) -> None:
        self.conn.execute(
            "INSERT INTO streamer_plans (twitch_login, promo_message) VALUES (?, ?)",
            ("partner_one", "Streamer Override {invite}"),
        )
        save_global_promo_mode(
            self.conn,
            config={
                "mode": "custom_event",
                "custom_message": "Global Event ohne Invite",
                "is_enabled": True,
            },
            updated_by="discord:55",
        )

        with self._conn_patch(), patch(
            "bot.chat.promos.PROMO_MESSAGES",
            ["Default Fallback {invite}"],
        ), patch(
            "bot.chat.promos.PROMO_MESSAGES_CATEGORIZED",
            {},
        ), patch(
            "bot.chat.promos.PROMO_CHANNEL_ALLOWLIST",
            [],
        ):
            ok = await self.handler._send_promo_message(
                "partner_one",
                "1001",
                0.0,
                reason="chat_activity",
            )

        self.assertTrue(ok)
        self.assertEqual(self.handler.announcement_calls[0]["text"], "Global Event ohne Invite")

    async def test_sent_promo_logs_channel_id_without_login(self) -> None:
        self.handler._last_promo_sent["partner_one"] = 0.0
        with self._conn_patch(), patch(
            "bot.chat.promos.PROMO_MESSAGES",
            ["Default Fallback {invite}"],
        ), patch(
            "bot.chat.promos.PROMO_MESSAGES_CATEGORIZED",
            {},
        ), patch(
            "bot.chat.promos.PROMO_CHANNEL_ALLOWLIST",
            [],
        ), patch(
            "bot.chat.promos._PROMO_ACTIVITY_ENABLED",
            False,
        ), patch(
            "bot.chat.promos.PROMO_VIEWER_SPIKE_ENABLED",
            False,
        ), patch(
            "bot.chat.promos.time.monotonic",
            return_value=10_000.0,
        ), patch(
            "bot.core.partner_utils.is_partner_channel_for_chat_tracking",
            return_value=True,
        ), patch.object(
            self.handler,
            "_get_live_channels_for_promo",
            AsyncMock(return_value=[("partner_one", "1001")]),
        ), patch.object(
            self.handler,
            "_get_live_channels_for_lurker_tax",
            AsyncMock(return_value=[]),
        ), self.assertLogs("TwitchStreams.ChatBot", level="INFO") as captured:
            await self.handler._send_promo_if_due()

        combined = "\n".join(captured.output)
        self.assertIn("channel_id=1001", combined)
        self.assertNotIn("partner_one", combined)
        self.assertEqual(
            self.handler.announcement_calls[0]["text"],
            "Default Fallback https://discord.gg/example",
        )

    async def test_inactive_global_event_keeps_streamer_override(self) -> None:
        self.conn.execute(
            "INSERT INTO streamer_plans (twitch_login, promo_message) VALUES (?, ?)",
            ("partner_one", "Streamer Override {invite}"),
        )
        save_global_promo_mode(
            self.conn,
            config={
                "mode": "custom_event",
                "custom_message": "Expired Event {invite}",
                "is_enabled": True,
                "ends_at": "2020-03-06T20:00:00+00:00",
            },
            updated_by="discord:55",
        )

        with self._conn_patch(), patch(
            "bot.chat.promos.PROMO_MESSAGES",
            ["Default Fallback {invite}"],
        ), patch(
            "bot.chat.promos.PROMO_MESSAGES_CATEGORIZED",
            {},
        ), patch(
            "bot.chat.promos.PROMO_CHANNEL_ALLOWLIST",
            [],
        ):
            ok = await self.handler._send_promo_message(
                "partner_one",
                "1001",
                0.0,
                reason="chat_activity",
            )

        self.assertTrue(ok)
        self.assertEqual(
            self.handler.announcement_calls[0]["text"],
            "Streamer Override https://discord.gg/example",
        )

    async def test_without_any_override_falls_back_to_default_messages(self) -> None:
        with self._conn_patch(), patch(
            "bot.chat.promos.PROMO_MESSAGES",
            ["Default Fallback {invite}"],
        ), patch(
            "bot.chat.promos.PROMO_MESSAGES_CATEGORIZED",
            {},
        ), patch(
            "bot.chat.promos.PROMO_CHANNEL_ALLOWLIST",
            [],
        ):
            ok = await self.handler._send_promo_message(
                "partner_one",
                "1001",
                0.0,
                reason="chat_activity",
            )

        self.assertTrue(ok)
        self.assertEqual(
            self.handler.announcement_calls[0]["text"],
            "Default Fallback https://discord.gg/example",
        )

    async def test_invalid_streamer_override_without_invite_falls_back_to_default(self) -> None:
        self.conn.execute(
            "INSERT INTO streamer_plans (twitch_login, promo_message) VALUES (?, ?)",
            ("partner_one", "Streamer Override ohne Invite"),
        )

        with self._conn_patch(), patch(
            "bot.chat.promos.PROMO_MESSAGES",
            ["Default Fallback {invite}"],
        ), patch(
            "bot.chat.promos.PROMO_MESSAGES_CATEGORIZED",
            {},
        ), patch(
            "bot.chat.promos.PROMO_CHANNEL_ALLOWLIST",
            [],
        ):
            ok = await self.handler._send_promo_message(
                "partner_one",
                "1001",
                0.0,
                reason="chat_activity",
            )

        self.assertTrue(ok)
        self.assertEqual(
            self.handler.announcement_calls[0]["text"],
            "Default Fallback https://discord.gg/example",
        )


if __name__ == "__main__":
    unittest.main()
