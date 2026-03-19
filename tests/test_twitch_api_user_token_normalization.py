import unittest
import time

from bot.api.twitch_api import TwitchAPI


class _FakeResponse:
    def __init__(self, *, status: int = 200, payload: dict | None = None, text: str = "") -> None:
        self.status = int(status)
        self._payload = dict(payload or {})
        self._text = text
        self.history = ()
        self.headers = {}
        self.reason = "Bad Request"
        self.request_info = None

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc, tb):
        return False

    async def json(self) -> dict:
        return dict(self._payload)

    async def text(self) -> str:
        return self._text

    def raise_for_status(self) -> None:
        raise RuntimeError(f"unexpected raise_for_status for HTTP {self.status}")


class _RecordingSession:
    def __init__(self, responses: list[_FakeResponse] | None = None) -> None:
        self._responses = list(responses or [])
        self.closed = False
        self.calls: list[dict[str, object]] = []

    def get(self, url, *, headers=None, params=None):
        self.calls.append(
            {
                "url": url,
                "headers": dict(headers or {}),
                "params": dict(params or {}),
            }
        )
        if not self._responses:
            raise AssertionError("No fake response configured")
        return self._responses.pop(0)


class TwitchApiUserTokenNormalizationTests(unittest.IsolatedAsyncioTestCase):
    async def test_get_followers_total_strips_oauth_prefix_from_user_token(self) -> None:
        session = _RecordingSession(
            responses=[_FakeResponse(payload={"total": 42, "data": []})]
        )
        api = TwitchAPI("client-id", "client-secret", session=session)

        total = await api.get_followers_total("1001", user_token="oauth:test-user-token")

        self.assertEqual(total, 42)
        self.assertEqual(
            session.calls[0]["headers"]["Authorization"],
            "Bearer test-user-token",
        )

    async def test_get_broadcaster_subscriptions_strips_oauth_prefix_from_user_token(self) -> None:
        session = _RecordingSession(
            responses=[_FakeResponse(payload={"data": [{"tier": "1000"}], "total": 1})]
        )
        api = TwitchAPI("client-id", "client-secret", session=session)

        payload = await api.get_broadcaster_subscriptions(
            "1001",
            user_token="oauth:test-sub-token",
        )

        self.assertEqual(payload, {"data": [{"tier": "1000"}], "total": 1})
        self.assertEqual(
            session.calls[0]["headers"]["Authorization"],
            "Bearer test-sub-token",
        )

    async def test_get_chatters_strips_oauth_prefix_from_user_token(self) -> None:
        session = _RecordingSession(
            responses=[
                _FakeResponse(
                    payload={
                        "data": [{"user_id": "55", "user_login": "viewer55"}],
                        "pagination": {},
                    }
                )
            ]
        )
        api = TwitchAPI("client-id", "client-secret", session=session)

        chatters = await api.get_chatters(
            broadcaster_id="1001",
            moderator_id="9999",
            user_token="oauth:test-chatters-token",
        )

        self.assertEqual(chatters, [{"user_id": "55", "user_login": "viewer55"}])
        self.assertEqual(
            session.calls[0]["headers"]["Authorization"],
            "Bearer test-chatters-token",
        )

    async def test_get_chatters_returns_none_when_followup_page_fails(self) -> None:
        session = _RecordingSession(
            responses=[
                _FakeResponse(
                    payload={
                        "data": [{"user_id": "55", "user_login": "viewer55"}],
                        "pagination": {"cursor": "next-page"},
                    }
                ),
                _FakeResponse(status=500, text="server exploded"),
            ]
        )
        api = TwitchAPI("client-id", "client-secret", session=session)

        chatters = await api.get_chatters(
            broadcaster_id="1001",
            moderator_id="9999",
            user_token="oauth:test-chatters-token",
        )

        self.assertIsNone(chatters)

    async def test_list_eventsub_subscriptions_returns_empty_when_followup_page_fails(self) -> None:
        session = _RecordingSession(
            responses=[
                _FakeResponse(
                    payload={
                        "data": [{"id": "sub-1", "status": "enabled"}],
                        "pagination": {"cursor": "next-page"},
                    }
                ),
                _FakeResponse(status=500, text="server exploded"),
            ]
        )
        api = TwitchAPI("client-id", "client-secret", session=session)
        api._token = "app-token"
        api._token_expiry = time.time() + 3600

        subscriptions = await api.list_eventsub_subscriptions(status="enabled")

        self.assertEqual(subscriptions, [])
