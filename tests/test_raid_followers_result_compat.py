import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from bot.raid.bot import RaidBot


class _AuthManagerStub:
    client_id = "client-id"
    client_secret = "client-secret"

    async def get_valid_token(self, user_id: str, session) -> str | None:
        return None


class _LegacyTwitchAPIStub:
    next_total = 321
    created_instances = []

    def __init__(self, client_id: str, client_secret: str, session=None) -> None:
        self.calls: list[tuple[str, str | None]] = []
        self.__class__.created_instances.append(self)

    async def get_followers_total(self, user_id: str, user_token: str | None = None) -> int | None:
        self.calls.append((user_id, user_token))
        return self.__class__.next_total


class RaidFollowersResultCompatTests(unittest.IsolatedAsyncioTestCase):
    def _make_bot(self) -> RaidBot:
        bot = object.__new__(RaidBot)
        bot.auth_manager = _AuthManagerStub()
        bot._session = SimpleNamespace(closed=False)
        bot._user_scope_fallback_warned = set()
        bot._build_analytics_followers_runtime_state = lambda: {}
        bot._log_analytics_followers_decision = lambda **kwargs: None
        bot._resolve_bot_oauth_context = AsyncMock(
            return_value=("bot-token", "9999", {"moderator:read:followers"})
        )
        return bot

    async def test_recruitment_uses_legacy_followers_api_when_structured_method_is_missing(self) -> None:
        bot = self._make_bot()
        _LegacyTwitchAPIStub.next_total = 654
        _LegacyTwitchAPIStub.created_instances = []

        with patch("bot.api.twitch_api.TwitchAPI", _LegacyTwitchAPIStub):
            result = await bot._resolve_recruitment_followers_total(
                login="targetlogin",
                target_id="1001",
                target_stream_data={},
            )

        self.assertEqual(result, 654)
        self.assertEqual(_LegacyTwitchAPIStub.created_instances[0].calls, [("1001", "bot-token")])

    async def test_candidate_uses_legacy_followers_api_when_structured_method_is_missing(self) -> None:
        bot = self._make_bot()
        _LegacyTwitchAPIStub.next_total = 987
        _LegacyTwitchAPIStub.created_instances = []

        with patch("bot.api.twitch_api.TwitchAPI", _LegacyTwitchAPIStub):
            candidates = [{"user_id": "2002", "user_login": "candidate_a", "followers_total": None}]
            await bot._attach_followers_totals(candidates)

        self.assertEqual(candidates[0]["followers_total"], 987)
        self.assertEqual(_LegacyTwitchAPIStub.created_instances[0].calls, [("2002", "bot-token")])


if __name__ == "__main__":
    unittest.main()
