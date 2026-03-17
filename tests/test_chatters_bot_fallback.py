import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock

from bot.analytics.mixin import TwitchAnalyticsMixin


class _AuthManager:
    def __init__(self, scopes_by_user: dict[str, list[str]] | None = None) -> None:
        self._scopes_by_user = scopes_by_user or {}

    def get_scopes(self, twitch_user_id: str) -> list[str]:
        return list(self._scopes_by_user.get(str(twitch_user_id), []))


class _BotTokenManager:
    def __init__(
        self,
        *,
        token: str = "bot-token",
        bot_id: str = "9999",
        scopes: set[str] | None = None,
    ) -> None:
        self._token = token
        self._bot_id = bot_id
        self.scopes = set(scopes or set())

    async def get_valid_token(self, force_refresh: bool = False) -> tuple[str, str]:
        return self._token, self._bot_id


class _ChatBot:
    def __init__(self, *, bot_id: str = "9999", monitored: set[str] | None = None) -> None:
        self.bot_id = bot_id
        self._bot_id_stored = bot_id
        self._monitored_streamers = set(monitored or set())

    @property
    def bot_id_safe(self) -> str:
        return self._bot_id_stored


class _AnalyticsHarness(TwitchAnalyticsMixin):
    def __init__(
        self,
        *,
        streamer_scopes: dict[str, list[str]] | None = None,
        bot_scopes: set[str] | None = None,
        monitored: set[str] | None = None,
    ) -> None:
        self.api = SimpleNamespace(get_chatters=AsyncMock())
        self._raid_bot = SimpleNamespace(auth_manager=_AuthManager(streamer_scopes))
        self._bot_token_manager = _BotTokenManager(scopes=bot_scopes)
        self._twitch_chat_bot = _ChatBot(monitored=monitored or {"partner_one"})
        self._chatters_scope_warned: set[tuple[str, int]] = set()


class ChattersBotFallbackTests(unittest.IsolatedAsyncioTestCase):
    async def test_poll_chatters_prefers_bot_scope_when_streamer_scope_is_also_available(self) -> None:
        harness = _AnalyticsHarness(
            streamer_scopes={"1001": ["moderator:read:chatters"]},
            bot_scopes={"moderator:read:chatters"},
        )
        harness.api.get_chatters.return_value = [{"user_login": "lurker_a", "user_id": "42"}]

        result = await harness._poll_chatters_single(
            "1001",
            "partner_one",
            77,
            "2026-03-15T10:00:00+00:00",
            token="streamer-token",
        )

        self.assertEqual(result, (77, "partner_one", [{"user_login": "lurker_a", "user_id": "42"}]))
        harness.api.get_chatters.assert_awaited_once_with(
            broadcaster_id="1001",
            moderator_id="9999",
            user_token="bot-token",
        )

    async def test_poll_chatters_falls_back_to_bot_scope_when_streamer_scope_missing(self) -> None:
        harness = _AnalyticsHarness(
            streamer_scopes={"1001": ["chat:read"]},
            bot_scopes={"moderator:read:chatters"},
        )
        harness.api.get_chatters.return_value = [{"user_login": "lurker_b", "user_id": "84"}]

        result = await harness._poll_chatters_single(
            "1001",
            "partner_one",
            88,
            "2026-03-15T10:00:00+00:00",
            token="streamer-token",
        )

        self.assertEqual(result, (88, "partner_one", [{"user_login": "lurker_b", "user_id": "84"}]))
        harness.api.get_chatters.assert_awaited_once_with(
            broadcaster_id="1001",
            moderator_id="9999",
            user_token="bot-token",
        )
        self.assertEqual(harness._chatters_scope_warned, set())

    async def test_poll_chatters_uses_streamer_scope_as_legacy_fallback_when_bot_is_unavailable(self) -> None:
        harness = _AnalyticsHarness(
            streamer_scopes={"1001": ["moderator:read:chatters"]},
            bot_scopes={"moderator:read:chatters"},
            monitored={"other_channel"},
        )
        harness.api.get_chatters.return_value = [{"user_login": "lurker_c", "user_id": "21"}]

        result = await harness._poll_chatters_single(
            "1001",
            "partner_one",
            89,
            "2026-03-15T10:00:00+00:00",
            token="streamer-token",
        )

        self.assertEqual(result, (89, "partner_one", [{"user_login": "lurker_c", "user_id": "21"}]))
        harness.api.get_chatters.assert_awaited_once_with(
            broadcaster_id="1001",
            moderator_id="1001",
            user_token="streamer-token",
        )

    async def test_poll_chatters_returns_none_when_neither_streamer_nor_bot_have_scope(self) -> None:
        harness = _AnalyticsHarness(
            streamer_scopes={"1001": ["chat:read"]},
            bot_scopes={"user:read:chat"},
        )

        result = await harness._poll_chatters_single(
            "1001",
            "partner_one",
            99,
            "2026-03-15T10:00:00+00:00",
            token="streamer-token",
        )

        self.assertIsNone(result)
        harness.api.get_chatters.assert_not_awaited()
        self.assertEqual(harness._chatters_scope_warned, {("1001", 99)})
