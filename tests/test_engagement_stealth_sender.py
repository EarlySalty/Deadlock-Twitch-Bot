import asyncio
import unittest
from unittest import mock

from bot.engagement import stealth_sender


class _FakeResp:
    def __init__(self, status, body):
        self.status = status
        self._body = body

    async def __aenter__(self):
        return self

    async def __aexit__(self, *a):
        return False

    async def text(self):
        return self._body


class _FakeSession:
    def __init__(self, captured, status, body):
        self._captured = captured
        self._status = status
        self._body = body

    async def __aenter__(self):
        return self

    async def __aexit__(self, *a):
        return False

    def post(self, url, headers=None, json=None):
        self._captured["url"] = url
        self._captured["headers"] = headers
        self._captured["json"] = json
        return _FakeResp(self._status, self._body)


def _session_factory(captured, status, body):
    def _make():
        return _FakeSession(captured, status, body)
    return _make


class StealthSenderTests(unittest.TestCase):
    def setUp(self):
        self._creds_patch = mock.patch.object(
            stealth_sender, "_client_credentials", return_value=("cid123", "secret")
        )
        self._creds_patch.start()
        self.addCleanup(self._creds_patch.stop)

    def test_returns_none_when_no_account(self):
        with mock.patch.object(stealth_sender, "get_valid_access_token", new=mock.AsyncMock(return_value=None)):
            result = asyncio.run(stealth_sender.send("999", "hi"))
        self.assertIsNone(result)

    def test_empty_text_returns_false(self):
        result = asyncio.run(stealth_sender.send("999", "   "))
        self.assertFalse(result)

    def test_sends_with_smoke_identity_and_returns_true(self):
        captured = {}
        body = '{"data":[{"message_id":"x","is_sent":true}]}'
        with mock.patch.object(
            stealth_sender, "get_valid_access_token",
            new=mock.AsyncMock(return_value=("tok_abc", "smoke_uid_42")),
        ), mock.patch.object(
            stealth_sender.aiohttp, "ClientSession", _session_factory(captured, 200, body)
        ):
            result = asyncio.run(stealth_sender.send("broadcaster_77", "ggwp"))
        self.assertTrue(result)
        # sender_id ist die Smoke-Identität, nicht der Haupt-Bot
        self.assertEqual(captured["json"]["sender_id"], "smoke_uid_42")
        self.assertEqual(captured["json"]["broadcaster_id"], "broadcaster_77")
        self.assertEqual(captured["json"]["message"], "ggwp")
        self.assertEqual(captured["headers"]["Authorization"], "Bearer tok_abc")

    def test_is_sent_false_returns_false(self):
        captured = {}
        body = '{"data":[{"is_sent":false,"drop_reason":{"code":"x"}}]}'
        with mock.patch.object(
            stealth_sender, "get_valid_access_token",
            new=mock.AsyncMock(return_value=("tok", "uid")),
        ), mock.patch.object(
            stealth_sender.aiohttp, "ClientSession", _session_factory(captured, 200, body)
        ):
            result = asyncio.run(stealth_sender.send("b", "msg"))
        self.assertFalse(result)

    def test_http_error_returns_false(self):
        captured = {}
        with mock.patch.object(
            stealth_sender, "get_valid_access_token",
            new=mock.AsyncMock(return_value=("tok", "uid")),
        ), mock.patch.object(
            stealth_sender.aiohttp, "ClientSession", _session_factory(captured, 401, "unauthorized")
        ):
            result = asyncio.run(stealth_sender.send("b", "msg"))
        self.assertFalse(result)


if __name__ == "__main__":
    unittest.main()
