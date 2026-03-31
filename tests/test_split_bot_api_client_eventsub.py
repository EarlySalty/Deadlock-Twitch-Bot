from __future__ import annotations

import unittest

from bot.dashboard_service.client import BotApiClient, BotApiClientError


class _FakeResponse:
    def __init__(self, *, status: int, text: str) -> None:
        self.status = int(status)
        self._text = text
        self.released = False

    async def text(self) -> str:
        return self._text

    def release(self) -> None:
        self.released = True


class _FakeSession:
    def __init__(self, *, response: _FakeResponse | None = None, exc: Exception | None = None) -> None:
        self.response = response
        self.exc = exc
        self.calls: list[dict[str, object]] = []
        self.closed = False

    async def request(self, method: str, url: str, **kwargs):
        self.calls.append({"method": method, "url": url, "kwargs": kwargs})
        if self.exc is not None:
            raise self.exc
        return self.response

    async def close(self) -> None:
        self.closed = True


class BotApiClientEventSubDispatchTests(unittest.IsolatedAsyncioTestCase):
    async def test_dispatch_eventsub_notification_uses_expected_endpoint_and_payload(self) -> None:
        session = _FakeSession(response=_FakeResponse(status=200, text='{"ok":true,"message":"queued"}'))
        client = BotApiClient(
            base_url="http://127.0.0.1:8766",
            token="secret",
            session=session,
        )

        payload = await client.dispatch_eventsub_notification(
            sub_type="stream.offline",
            message_id="  msg-1  ",
            payload={"event": {"broadcaster_user_id": "42"}},
        )

        self.assertEqual(payload["ok"], True)
        self.assertEqual(len(session.calls), 1)
        self.assertEqual(
            session.calls[0]["url"],
            "http://127.0.0.1:8766/internal/twitch/v1/eventsub/dispatch",
        )
        self.assertEqual(
            session.calls[0]["kwargs"]["json"],
            {
                "sub_type": "stream.offline",
                "message_id": "msg-1",
                "payload": {"event": {"broadcaster_user_id": "42"}},
            },
        )
        self.assertFalse(session.calls[0]["kwargs"]["allow_redirects"])

    async def test_dispatch_eventsub_notification_raises_safe_error_on_false_ok(self) -> None:
        session = _FakeSession(response=_FakeResponse(status=200, text='{"ok":false}'))
        client = BotApiClient(
            base_url="http://127.0.0.1:8766",
            token="secret",
            session=session,
        )

        with self.assertRaises(BotApiClientError) as ctx:
            await client.dispatch_eventsub_notification(
                sub_type="stream.offline",
                message_id="msg-2",
                payload={"event": {"broadcaster_user_id": "42"}},
            )

        self.assertEqual(ctx.exception.status, 503)
        self.assertEqual(ctx.exception.code, "upstream_unavailable")
        self.assertEqual(
            ctx.exception.message,
            "Bot internal API could not dispatch the EventSub notification.",
        )

    async def test_dispatch_eventsub_notification_rejects_non_object_response_shape(self) -> None:
        session = _FakeSession(response=_FakeResponse(status=200, text="[]"))
        client = BotApiClient(
            base_url="http://127.0.0.1:8766",
            token="secret",
            session=session,
        )

        with self.assertRaises(BotApiClientError) as ctx:
            await client.dispatch_eventsub_notification(
                sub_type="stream.offline",
                message_id="msg-3",
                payload={"event": {"broadcaster_user_id": "42"}},
            )

        self.assertEqual(ctx.exception.status, 502)
        self.assertEqual(ctx.exception.code, "upstream_invalid_shape")
        self.assertEqual(
            ctx.exception.message,
            "Bot internal API returned an invalid EventSub dispatch payload.",
        )
        self.assertTrue(session.response.released)

    async def test_dispatch_eventsub_notification_rejects_non_object_response_shape(self) -> None:
        session = _FakeSession(response=_FakeResponse(status=200, text="[]"))
        client = BotApiClient(
            base_url="http://127.0.0.1:8766",
            token="secret",
            session=session,
        )

        with self.assertRaises(BotApiClientError) as ctx:
            await client.dispatch_eventsub_notification(
                sub_type="stream.offline",
                message_id="msg-3",
                payload={"event": {"broadcaster_user_id": "42"}},
            )

        self.assertEqual(ctx.exception.status, 502)
        self.assertEqual(ctx.exception.code, "upstream_invalid_shape")
        self.assertEqual(
            ctx.exception.message,
            "Bot internal API returned an invalid EventSub dispatch payload.",
        )
        self.assertEqual(session.calls[0]["kwargs"]["allow_redirects"], False)
        self.assertTrue(session.response.released)
