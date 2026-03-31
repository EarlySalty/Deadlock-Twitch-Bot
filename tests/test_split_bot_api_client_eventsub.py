from __future__ import annotations

import asyncio
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
    def __init__(
        self,
        *,
        response: _FakeResponse | None = None,
        exc: Exception | None = None,
    ) -> None:
        self._response = response
        self._exc = exc
        self.closed = False
        self.calls: list[dict[str, object]] = []

    async def request(self, method: str, url: str, **kwargs):
        self.calls.append({"method": method, "url": url, "kwargs": kwargs})
        if self._exc is not None:
            raise self._exc
        return self._response

    async def close(self) -> None:
        self.closed = True


class BotApiClientEventSubTests(unittest.IsolatedAsyncioTestCase):
    async def test_dispatch_eventsub_notification_trims_request_fields_and_forwards_payload(self) -> None:
        session = _FakeSession(response=_FakeResponse(status=200, text='{"ok":true}'))
        client = BotApiClient(
            base_url="http://127.0.0.1:8766",
            token="secret",
            session=session,
        )

        payload = {"subscription": {"type": "stream.offline"}, "event": {"broadcaster_user_id": "42"}}
        response = await client.dispatch_eventsub_notification(
            sub_type=" stream.offline ",
            message_id="   ",
            payload=payload,
        )

        self.assertTrue(response["ok"])
        self.assertEqual(len(session.calls), 1)
        self.assertEqual(
            session.calls[0]["kwargs"]["json"],
            {
                "sub_type": "stream.offline",
                "message_id": None,
                "payload": payload,
            },
        )

    async def test_dispatch_eventsub_notification_uses_generic_fallback_when_upstream_omits_message(self) -> None:
        session = _FakeSession(response=_FakeResponse(status=200, text='{"ok":false}'))
        client = BotApiClient(
            base_url="http://127.0.0.1:8766",
            token="secret",
            session=session,
        )

        with self.assertRaises(BotApiClientError) as ctx:
            await client.dispatch_eventsub_notification(
                sub_type="stream.offline",
                message_id="msg-1",
                payload={"event": {"broadcaster_user_id": "42"}},
            )

        self.assertEqual(ctx.exception.status, 503)
        self.assertEqual(ctx.exception.code, "upstream_unavailable")
        self.assertIn("could not dispatch", ctx.exception.message)

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
                message_id="msg-2",
                payload={"event": {"broadcaster_user_id": "42"}},
            )

        self.assertEqual(ctx.exception.status, 502)
        self.assertEqual(ctx.exception.code, "upstream_invalid_shape")
        self.assertEqual(
            ctx.exception.message,
            "Bot internal API returned an invalid EventSub dispatch payload.",
        )
        self.assertTrue(session.calls[0]["kwargs"]["allow_redirects"] is False)
        self.assertTrue(session._response.released)
