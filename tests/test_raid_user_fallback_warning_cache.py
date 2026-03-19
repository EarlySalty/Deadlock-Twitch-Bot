import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from bot.raid.bot import RaidBot


class _AuthManagerStub:
    client_id = "client-id"
    client_secret = "client-secret"

    async def get_valid_token(self, user_id: str, session) -> str | None:
        return None


class _TwitchAPIStub:
    next_result = {"ok": True, "data": 321, "http_status": 200}
    created_instances = []

    def __init__(self, client_id: str, client_secret: str, session=None) -> None:
        self.client_id = client_id
        self.client_secret = client_secret
        self.session = session
        self.calls: list[tuple[str, str | None]] = []
        self.__class__.created_instances.append(self)

    async def get_followers_total_result(self, user_id: str, user_token: str | None = None) -> dict:
        self.calls.append((user_id, user_token))
        return dict(self.__class__.next_result)


class RaidUserFallbackWarningCacheTests(unittest.IsolatedAsyncioTestCase):
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

    async def test_recruitment_bot_success_clears_stale_user_fallback_warning(self) -> None:
        bot = self._make_bot()
        bot._user_scope_fallback_warned.add(("recruitment follower lookup", "targetlogin"))
        _TwitchAPIStub.next_result = {"ok": True, "data": 654, "http_status": 200}
        _TwitchAPIStub.created_instances = []

        with patch("bot.api.twitch_api.TwitchAPI", _TwitchAPIStub):
            result = await bot._resolve_recruitment_followers_total(
                login="targetlogin",
                target_id="1001",
                target_stream_data={},
            )

        self.assertEqual(result, 654)
        self.assertNotIn(
            ("recruitment follower lookup", "targetlogin"),
            bot._user_scope_fallback_warned,
        )
        self.assertEqual(_TwitchAPIStub.created_instances[0].calls, [("1001", "bot-token")])

    async def test_candidate_bot_success_clears_stale_user_fallback_warning(self) -> None:
        bot = self._make_bot()
        bot._user_scope_fallback_warned.add(("raid candidate follower lookup", "candidate_a"))
        _TwitchAPIStub.next_result = {"ok": True, "data": 987, "http_status": 200}
        _TwitchAPIStub.created_instances = []

        with patch("bot.api.twitch_api.TwitchAPI", _TwitchAPIStub):
            candidates = [{"user_id": "2002", "user_login": "candidate_a", "followers_total": None}]
            await bot._attach_followers_totals(candidates)

        self.assertEqual(candidates[0]["followers_total"], 987)
        self.assertNotIn(
            ("raid candidate follower lookup", "candidate_a"),
            bot._user_scope_fallback_warned,
        )
        self.assertEqual(_TwitchAPIStub.created_instances[0].calls, [("2002", "bot-token")])


if __name__ == "__main__":
    unittest.main()
