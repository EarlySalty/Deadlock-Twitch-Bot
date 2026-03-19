import unittest

from bot.monitoring.sessions_mixin import _SessionsMixin


class _ApiStub:
    def __init__(self, followers_total: int | None) -> None:
        self._followers_total = followers_total
        self.calls: list[tuple[str, str | None]] = []

    async def get_followers_total(self, user_id: str, user_token: str | None = None) -> int | None:
        self.calls.append((str(user_id), str(user_token) if user_token is not None else None))
        return self._followers_total


class _BotTokenManager:
    def __init__(self, *, token: str = "bot-token", scopes: set[str] | None = None) -> None:
        self._token = token
        self.scopes = set(scopes or set())

    async def get_valid_token(self):
        return self._token, "9999"


class _FollowersHarness(_SessionsMixin):
    def __init__(self, *, followers_total: int | None, scopes: set[str] | None = None) -> None:
        self.api = _ApiStub(followers_total)
        self._bot_token_manager = _BotTokenManager(scopes=scopes)
        self._session_followers_user_fallback_warned = {"partner_one"}
        self._raid_bot = None


class SessionFollowersFallbackCacheTests(unittest.IsolatedAsyncioTestCase):
    async def test_bot_success_clears_stale_fallback_warning_cache_for_login(self) -> None:
        harness = _FollowersHarness(
            followers_total=1234,
            scopes={"moderator:read:followers"},
        )

        total = await harness._fetch_followers_total_safe(
            twitch_user_id="1001",
            login="partner_one",
            stream=None,
        )

        self.assertEqual(total, 1234)
        self.assertEqual(harness._session_followers_user_fallback_warned, set())
        self.assertEqual(harness.api.calls, [("1001", "bot-token")])


if __name__ == "__main__":
    unittest.main()
