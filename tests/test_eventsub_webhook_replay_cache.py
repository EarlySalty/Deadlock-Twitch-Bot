import asyncio
import hashlib
import hmac
import json
import unittest
from datetime import UTC, datetime
from unittest.mock import patch
from types import SimpleNamespace

from bot.monitoring.eventsub_webhook import EventSubWebhookHandler, _MAX_MESSAGE_AGE_SECONDS


class EventSubWebhookReplayCacheTests(unittest.TestCase):
    def test_replay_ids_are_not_evicted_by_count_within_ttl_window(self) -> None:
        handler = EventSubWebhookHandler(secret="test-secret")
        clock = {"now": 1_000.0}
        handler._now_timestamp = lambda: clock["now"]  # type: ignore[method-assign]

        for idx in range(2_500):
            self.assertTrue(handler._track_message_id(f"msg-{idx}"))

        self.assertTrue(handler._is_duplicate("msg-0"))

    def test_expired_ids_are_cleaned_up(self) -> None:
        handler = EventSubWebhookHandler(secret="test-secret")
        clock = {"now": 5_000.0}
        handler._now_timestamp = lambda: clock["now"]  # type: ignore[method-assign]

        self.assertTrue(handler._track_message_id("msg-a"))
        self.assertTrue(handler._track_message_id("msg-b"))
        self.assertEqual(len(handler._seen_message_ids), 2)

        clock["now"] += _MAX_MESSAGE_AGE_SECONDS + 1

        self.assertFalse(handler._is_duplicate("msg-a"))
        self.assertEqual(handler._seen_message_ids, {})
        self.assertEqual(handler._seen_expiry_heap, [])

    @patch("bot.monitoring.eventsub_webhook._SEEN_ID_HARD_LIMIT", 2)
    def test_optional_hard_limit_is_fail_closed_without_ttl_eviction(self) -> None:
        handler = EventSubWebhookHandler(secret="test-secret")
        clock = {"now": 10_000.0}
        handler._now_timestamp = lambda: clock["now"]  # type: ignore[method-assign]

        self.assertTrue(handler._track_message_id("msg-1"))
        self.assertTrue(handler._track_message_id("msg-2"))
        self.assertFalse(handler._track_message_id("msg-3"))
        self.assertTrue(handler._is_duplicate("msg-1"))


class EventSubWebhookRequestValidationTests(unittest.IsolatedAsyncioTestCase):
    async def test_missing_twitch_headers_returns_400_without_warning(self) -> None:
        handler = EventSubWebhookHandler(secret="test-secret")
        request = SimpleNamespace(
            headers={},
            read=self._async_return(b"{}"),
        )

        with self.assertLogs("TwitchStreams.EventSubWebhook", level="INFO") as captured:
            response = await handler.handle_request(request)

        self.assertEqual(response.status, 400)
        self.assertTrue(
            any("ohne erforderliche Twitch-Header" in entry for entry in captured.output)
        )
        self.assertFalse(any("WARNING" in entry for entry in captured.output))

    async def test_invalid_signature_with_twitch_headers_stays_warning(self) -> None:
        handler = EventSubWebhookHandler(secret="test-secret")
        request = SimpleNamespace(
            headers={
                "Twitch-Eventsub-Message-Id": "msg-1",
                "Twitch-Eventsub-Message-Timestamp": "2026-03-29T05:01:41Z",
                "Twitch-Eventsub-Message-Signature": "sha256=deadbeef",
                "Twitch-Eventsub-Message-Type": "notification",
            },
            read=self._async_return(b"{}"),
        )

        with self.assertLogs("TwitchStreams.EventSubWebhook", level="WARNING") as captured:
            response = await handler.handle_request(request)

        self.assertEqual(response.status, 403)
        self.assertTrue(any("Signatur-Verifizierung fehlgeschlagen" in entry for entry in captured.output))

    async def test_stream_offline_notification_logs_accept_and_dispatch_lifecycle(self) -> None:
        handler = EventSubWebhookHandler(secret="test-secret")
        callback_calls: list[tuple[str, str, dict]] = []

        async def _offline_callback(broadcaster_id: str, broadcaster_login: str, event: dict) -> None:
            callback_calls.append((broadcaster_id, broadcaster_login, event))

        handler.set_callback("stream.offline", _offline_callback)

        body = {
            "subscription": {
                "type": "stream.offline",
                "condition": {"broadcaster_user_id": "457002490"},
            },
            "event": {
                "broadcaster_user_id": "457002490",
                "broadcaster_user_login": "tolgiziusx3",
            },
        }
        request = self._signed_request(
            secret="test-secret",
            body=body,
            message_id="msg-stream-offline-1",
        )

        with self.assertLogs("TwitchStreams.EventSubWebhook", level="INFO") as captured:
            response = await handler.handle_request(request)
            await asyncio.sleep(0)

        self.assertEqual(response.status, 204)
        self.assertEqual(len(callback_calls), 1)
        self.assertTrue(any("Notification accepted type='stream.offline'" in entry for entry in captured.output))
        self.assertTrue(any("Dispatch start type='stream.offline'" in entry for entry in captured.output))
        self.assertTrue(any("Dispatch completed type='stream.offline'" in entry for entry in captured.output))

    async def test_stream_offline_without_callback_logs_warning(self) -> None:
        handler = EventSubWebhookHandler(secret="test-secret")
        body = {
            "subscription": {
                "type": "stream.offline",
                "condition": {"broadcaster_user_id": "457002490"},
            },
            "event": {
                "broadcaster_user_id": "457002490",
                "broadcaster_user_login": "tolgiziusx3",
            },
        }

        with self.assertLogs("TwitchStreams.EventSubWebhook", level="WARNING") as captured:
            await handler._dispatch_notification(
                body,
                "stream.offline",
                message_id="msg-stream-offline-missing-callback",
            )

        self.assertTrue(any("Kein Callback für type='stream.offline'" in entry for entry in captured.output))

    @staticmethod
    def _async_return(value):
        async def _reader():
            return value

        return _reader

    @staticmethod
    def _signed_request(*, secret: str, body: dict, message_id: str) -> SimpleNamespace:
        raw_body = json.dumps(body).encode("utf-8")
        timestamp = datetime.now(UTC).isoformat().replace("+00:00", "Z")
        signature = hmac.new(
            secret.encode("utf-8"),
            message_id.encode("utf-8") + timestamp.encode("utf-8") + raw_body,
            hashlib.sha256,
        ).hexdigest()
        return SimpleNamespace(
            headers={
                "Twitch-Eventsub-Message-Id": message_id,
                "Twitch-Eventsub-Message-Timestamp": timestamp,
                "Twitch-Eventsub-Message-Signature": f"sha256={signature}",
                "Twitch-Eventsub-Message-Type": "notification",
                "Twitch-Eventsub-Subscription-Type": "stream.offline",
            },
            read=EventSubWebhookRequestValidationTests._async_return(raw_body),
        )


if __name__ == "__main__":
    unittest.main()
