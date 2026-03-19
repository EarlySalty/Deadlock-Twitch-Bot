import unittest
from unittest.mock import patch

from bot.api.token_manager import TwitchBotTokenManager


class _FakeResponse:
    def __init__(self, *, status: int, payload: dict) -> None:
        self.status = status
        self._payload = payload

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc, tb):
        return False

    async def json(self) -> dict:
        return dict(self._payload)


class _FakeSession:
    def __init__(self, responses: list[_FakeResponse]) -> None:
        self._responses = list(responses)

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc, tb):
        return False

    def get(self, url: str, headers=None):
        return self._responses.pop(0)


class TwitchBotTokenManagerTests(unittest.IsolatedAsyncioTestCase):
    async def test_validate_fetches_bot_login_from_validate_payload(self) -> None:
        manager = TwitchBotTokenManager("client-id", "client-secret")
        manager.access_token = "oauth:test-token"

        with patch(
            "bot.api.token_manager.aiohttp.ClientSession",
            return_value=_FakeSession(
                [
                    _FakeResponse(
                        status=200,
                        payload={
                            "user_id": "1234",
                            "login": "deadbot",
                            "scopes": ["user:read:chat", "moderator:read:chatters"],
                            "expires_in": 3600,
                        },
                    )
                ]
            ),
        ):
            ok = await manager._validate_and_fetch_info()

        self.assertTrue(ok)
        self.assertEqual(manager.bot_id, "1234")
        self.assertEqual(manager.bot_login, "deadbot")
        self.assertEqual(
            manager.scopes,
            {"user:read:chat", "moderator:read:chatters"},
        )


if __name__ == "__main__":
    unittest.main()
