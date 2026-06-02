from __future__ import annotations

import sqlite3
import unittest
from contextlib import ExitStack
from unittest.mock import patch

from bot.chat.constants import SCAM_WARNING_MESSAGES
from bot.chat.promos import PromoMixin


class _CompatConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    @staticmethod
    def _translate(sql: str) -> str:
        return str(sql).replace("%s", "?")

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
        self._raw_msg_count_since_promo: dict[str, int] = {}
        self._last_promo_viewer_spike: dict[str, float] = {}
        self._last_scam_warning_sent: dict[str, float] = {}
        self._promo_activity: dict = {}
        self._promo_seen_chatters: dict = {}
        self._promo_seen_chatters_ts: dict = {}

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


class ScamWarningConstantTests(unittest.TestCase):
    def test_messages_name_both_servers_and_use_invite(self) -> None:
        self.assertTrue(SCAM_WARNING_MESSAGES, "Es muss mindestens einen Warntext geben")
        for text in SCAM_WARNING_MESSAGES:
            self.assertIn("Deadlock Discord Deutschland", text)
            self.assertIn("Deadlock German Competitiv HUB", text)
            self.assertIn("{invite}", text)


class ScamWarningSendTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self.conn = sqlite3.connect(":memory:")
        self.conn.row_factory = sqlite3.Row
        # Leere streamer_plans-Tabelle -> _promo_blocked_by_plan_or_flag = False
        self.conn.execute(
            """
            CREATE TABLE streamer_plans (
                twitch_login TEXT PRIMARY KEY,
                twitch_user_id TEXT,
                promo_message TEXT,
                promo_disabled INTEGER NOT NULL DEFAULT 0,
                manual_plan_id TEXT,
                manual_plan_expires_at TEXT,
                manual_plan_notes TEXT NOT NULL DEFAULT '',
                manual_plan_updated_at TEXT
            )
            """
        )
        self.conn.execute(
            """
            CREATE TABLE twitch_billing_subscriptions (
                stripe_subscription_id TEXT PRIMARY KEY,
                customer_reference TEXT,
                status TEXT,
                plan_id TEXT,
                current_period_end TEXT,
                updated_at TEXT
            )
            """
        )
        self.handler = _DummyPromoChat()

    def tearDown(self) -> None:
        self.conn.close()

    def _base_patches(self) -> ExitStack:
        """Promo-Fallback deterministisch + Conn auf SQLite umbiegen."""
        stack = ExitStack()
        stack.enter_context(
            patch("bot.chat.promos.readonly_connection", return_value=_ConnCtx(self.conn))
        )
        stack.enter_context(
            patch("bot.chat.promos.transaction", return_value=_ConnCtx(self.conn))
        )
        stack.enter_context(patch("bot.chat.promos.PROMO_MESSAGES", ["Default Promo {invite}"]))
        stack.enter_context(patch("bot.chat.promos.PROMO_MESSAGES_CATEGORIZED", {}))
        stack.enter_context(patch("bot.chat.promos.PROMO_CHANNEL_ALLOWLIST", []))
        stack.enter_context(patch("bot.chat.promos.SCAM_WARNING_ENABLED", True))
        stack.enter_context(patch("bot.chat.promos.SCAM_WARNING_COOLDOWN_MIN", 45))
        stack.enter_context(
            patch(
                "bot.chat.promos.SCAM_WARNING_MESSAGES",
                [
                    "WARN Deadlock Discord Deutschland / Deadlock German Competitiv HUB {invite}",
                ],
            )
        )
        return stack

    async def test_first_opportunity_seeds_timer_and_sends_promo(self) -> None:
        # Erste Gelegenheit: nur Timer säen, normal werben (keine Warnung).
        with self._base_patches():
            ok = await self.handler._send_promo_message(
                "partner_one", "1001", 0.0, reason="chat_activity"
            )

        self.assertTrue(ok)
        self.assertEqual(self.handler.announcement_calls[0]["source"], "promo")
        self.assertEqual(
            self.handler.announcement_calls[0]["text"],
            "Default Promo https://discord.gg/example",
        )
        # Timer wurde gesät
        self.assertEqual(self.handler._last_scam_warning_sent["partner_one"], 0.0)

    async def test_warning_fires_after_cooldown_elapsed(self) -> None:
        # Timer vor 46 Minuten gesät -> jetzt fällig
        self.handler._last_scam_warning_sent["partner_one"] = 0.0
        with self._base_patches():
            ok = await self.handler._send_promo_message(
                "partner_one", "1001", 46 * 60.0, reason="chat_activity"
            )

        self.assertTrue(ok)
        call = self.handler.announcement_calls[0]
        self.assertEqual(
            call["text"],
            "WARN Deadlock Discord Deutschland / Deadlock German Competitiv HUB "
            "https://discord.gg/example",
        )
        self.assertEqual(call["color"], "orange")
        self.assertEqual(call["source"], "scam_warning")
        # Cooldown wurde auf den Sendezeitpunkt zurückgesetzt
        self.assertEqual(self.handler._last_scam_warning_sent["partner_one"], 46 * 60.0)

    async def test_cooldown_blocks_repeat_and_sends_normal_promo(self) -> None:
        self.handler._last_scam_warning_sent["partner_one"] = 0.0
        with self._base_patches():
            # 100s < 45min Cooldown -> Warnung gesperrt, normale Promo stattdessen
            ok = await self.handler._send_promo_message(
                "partner_one", "1001", 100.0, reason="chat_activity"
            )

        self.assertTrue(ok)
        self.assertEqual(
            self.handler.announcement_calls[0]["text"],
            "Default Promo https://discord.gg/example",
        )

    async def test_viewer_spike_never_triggers_warning(self) -> None:
        # Auch wenn der Cooldown längst fällig wäre: viewer_spike -> normale Promo
        self.handler._last_scam_warning_sent["partner_one"] = 0.0
        with self._base_patches():
            ok = await self.handler._send_promo_message(
                "partner_one", "1001", 5000.0, reason="viewer_spike"
            )

        self.assertTrue(ok)
        self.assertEqual(self.handler.announcement_calls[0]["source"], "promo")

    async def test_promo_disabled_blocks_warning_too(self) -> None:
        self.conn.execute(
            "INSERT INTO streamer_plans (twitch_login, promo_disabled) VALUES (?, ?)",
            ("partner_one", 1),
        )
        self.handler._last_scam_warning_sent["partner_one"] = 0.0
        with self._base_patches():
            ok = await self.handler._send_promo_message(
                "partner_one", "1001", 5000.0, reason="chat_activity"
            )

        self.assertFalse(ok)
        self.assertEqual(self.handler.announcement_calls, [])


if __name__ == "__main__":
    unittest.main()
