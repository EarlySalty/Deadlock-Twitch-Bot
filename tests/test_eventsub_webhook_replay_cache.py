import asyncio
import hashlib
import hmac
import json
import tempfile
import unittest
from datetime import UTC, datetime
from unittest.mock import patch
from types import SimpleNamespace

from bot.monitoring.eventsub_core_callbacks import register_core_eventsub_callbacks
from bot.monitoring.eventsub_state_store import EventSubStateStore
from bot.monitoring.eventsub_webhook import EventSubWebhookHandler, _MAX_MESSAGE_AGE_SECONDS
from tests.eventsub_state_store_test_helpers import InMemoryEventSubStateRepository


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

    async def test_future_timestamp_outside_replay_window_is_rejected(self) -> None:
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
        request = self._signed_request(
            secret="test-secret",
            body=body,
            message_id="msg-future-ts",
            timestamp="2100-01-01T00:00:00Z",
        )

        response = await handler.handle_request(request)

        self.assertEqual(response.status, 403)

    async def test_stream_offline_notification_logs_accept_and_dispatch_lifecycle(self) -> None:
        handler = EventSubWebhookHandler(secret="test-secret", synchronous_notifications=True)
        callback_calls: list[tuple[str, str, dict, str | None]] = []

        async def _offline_callback(
            broadcaster_id: str,
            broadcaster_login: str,
            event: dict,
            *,
            message_id: str | None = None,
        ) -> None:
            callback_calls.append((broadcaster_id, broadcaster_login, event, message_id))

        handler.set_callback("stream.offline", _offline_callback)
        handler.activate_notification_dispatch()

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

        self.assertEqual(response.status, 204)
        self.assertEqual(len(callback_calls), 1)
        self.assertEqual(callback_calls[0][3], "msg-stream-offline-1")
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
            with self.assertRaisesRegex(RuntimeError, "no callback registered"):
                await handler._dispatch_notification(
                    body,
                    "stream.offline",
                    message_id="msg-stream-offline-missing-callback",
                )

        self.assertTrue(any("Kein Callback für type='stream.offline'" in entry for entry in captured.output))

    async def test_notification_before_dispatch_activation_returns_503_without_tracking_message_id(
        self,
    ) -> None:
        handler = EventSubWebhookHandler(secret="test-secret", synchronous_notifications=True)
        body = {
            "subscription": {
                "type": "channel.raid",
                "condition": {"to_broadcaster_user_id": "457002490"},
            },
            "event": {
                "to_broadcaster_user_id": "457002490",
                "to_broadcaster_user_login": "tolgiziusx3",
            },
        }
        request = self._signed_request(
            secret="test-secret",
            body=body,
            message_id="msg-channel-raid-missing-callback-sync",
            subscription_type="channel.raid",
        )

        response = await handler.handle_request(request)

        self.assertEqual(response.status, 503)
        self.assertFalse(handler._is_duplicate("msg-channel-raid-missing-callback-sync"))

    async def test_notification_without_callback_returns_503_and_releases_message_id(self) -> None:
        handler = EventSubWebhookHandler(secret="test-secret")
        handler.activate_notification_dispatch()
        body = {
            "subscription": {
                "type": "channel.raid",
                "condition": {"to_broadcaster_user_id": "457002490"},
            },
            "event": {
                "to_broadcaster_user_id": "457002490",
                "to_broadcaster_user_login": "tolgiziusx3",
            },
        }
        request = self._signed_request(
            secret="test-secret",
            body=body,
            message_id="msg-channel-raid-missing-callback-async",
            subscription_type="channel.raid",
        )

        response = await handler.handle_request(request)
        await asyncio.sleep(0)
        await asyncio.sleep(0)

        self.assertEqual(response.status, 503)
        self.assertFalse(handler._is_duplicate("msg-channel-raid-missing-callback-async"))

    async def test_sync_notification_failure_returns_503_and_releases_message_id(self) -> None:
        handler = EventSubWebhookHandler(secret="test-secret", synchronous_notifications=True)

        async def _offline_callback(
            broadcaster_id: str,
            broadcaster_login: str,
            event: dict,
            *,
            message_id: str | None = None,
        ) -> None:
            del broadcaster_id, broadcaster_login, event, message_id
            raise RuntimeError("bridge unavailable")

        handler.set_callback("stream.offline", _offline_callback)
        handler.activate_notification_dispatch()
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
            message_id="msg-stream-offline-failure",
        )

        response = await handler.handle_request(request)

        self.assertEqual(response.status, 503)
        self.assertFalse(handler._is_duplicate("msg-stream-offline-failure"))

    async def test_registered_core_callbacks_propagate_sync_failures_to_503(self) -> None:
        handler = EventSubWebhookHandler(secret="test-secret", synchronous_notifications=True)

        class _FailingOwner:
            _raid_bot = None

            async def _on_eventsub_stream_offline(
                self,
                broadcaster_id: str,
                broadcaster_login: str,
                *,
                message_id: str | None = None,
            ) -> None:
                del broadcaster_id, broadcaster_login, message_id
                raise RuntimeError("offline persistence failed")

            async def _handle_stream_online(
                self,
                broadcaster_id: str,
                broadcaster_login: str,
                event: dict,
                *,
                message_id: str | None = None,
            ) -> None:
                del broadcaster_id, broadcaster_login, event, message_id

            async def _handle_channel_update(
                self,
                broadcaster_id: str,
                event: dict,
                *,
                message_id: str | None = None,
            ) -> None:
                del broadcaster_id, event, message_id

        register_core_eventsub_callbacks(
            _FailingOwner(),
            handler,
            propagate_callback_errors=True,
        )
        handler.activate_notification_dispatch()

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
            message_id="msg-stream-offline-core-failure",
        )

        response = await handler.handle_request(request)

        self.assertEqual(response.status, 503)
        self.assertFalse(handler._is_duplicate("msg-stream-offline-core-failure"))

    async def test_registered_core_callbacks_in_inline_mode_do_not_ack_after_enqueue_only(self) -> None:
        handler = EventSubWebhookHandler(secret="test-secret", synchronous_notifications=True)

        class _Owner:
            def __init__(self) -> None:
                self._raid_bot = None
                self._enqueue_eventsub_stream_online_processing = unittest.mock.AsyncMock(  # type: ignore[attr-defined]
                    return_value=None
                )
                self._handle_stream_online = unittest.mock.AsyncMock(  # type: ignore[attr-defined]
                    side_effect=RuntimeError("stream.online processing failed")
                )

            async def _on_eventsub_stream_offline(
                self,
                broadcaster_id: str,
                broadcaster_login: str,
            ) -> None:
                del broadcaster_id, broadcaster_login

            async def _handle_channel_update(self, broadcaster_id: str, event: dict) -> None:
                del broadcaster_id, event

        owner = _Owner()
        register_core_eventsub_callbacks(
            owner,
            handler,
            propagate_callback_errors=True,
            delivery_mode="inline",
        )
        handler.activate_notification_dispatch()

        body = {
            "subscription": {
                "type": "stream.online",
                "condition": {"broadcaster_user_id": "457002490"},
            },
            "event": {
                "broadcaster_user_id": "457002490",
                "broadcaster_user_login": "tolgiziusx3",
                "id": "stream-1",
                "type": "live",
            },
        }
        request = self._signed_request(
            secret="test-secret",
            body=body,
            message_id="msg-stream-online-inline-failure",
            subscription_type="stream.online",
        )

        response = await handler.handle_request(request)

        self.assertEqual(response.status, 503)
        owner._handle_stream_online.assert_awaited_once_with(
            "457002490",
            "tolgiziusx3",
            {
                "broadcaster_user_id": "457002490",
                "broadcaster_user_login": "tolgiziusx3",
                "id": "stream-1",
                "type": "live",
            },
            message_id="msg-stream-online-inline-failure",
        )
        owner._enqueue_eventsub_stream_online_processing.assert_not_awaited()
        self.assertFalse(handler._is_duplicate("msg-stream-online-inline-failure"))

    async def test_revocation_callback_is_invoked_with_payload_and_message_id(self) -> None:
        handler = EventSubWebhookHandler(secret="test-secret", synchronous_notifications=True)
        revocations: list[tuple[str, str | None]] = []

        async def _revocation_callback(payload: dict, *, message_id: str | None = None) -> None:
            revocations.append((str(payload.get("subscription", {}).get("type") or ""), message_id))

        handler.set_revocation_callback(_revocation_callback)
        body = {
            "subscription": {
                "type": "stream.offline",
                "status": "authorization_revoked",
                "condition": {"broadcaster_user_id": "457002490"},
            }
        }
        request = self._signed_request(
            secret="test-secret",
            body=body,
            message_id="msg-stream-offline-revoked",
            message_type="revocation",
        )

        response = await handler.handle_request(request)

        self.assertEqual(response.status, 204)
        self.assertEqual(revocations, [("stream.offline", "msg-stream-offline-revoked")])

    async def test_internal_dispatch_queues_once_and_deduplicates_message_id(self) -> None:
        handler = EventSubWebhookHandler(secret="test-secret")
        release = asyncio.Event()
        callback_calls: list[str] = []

        async def _follow_callback(
            broadcaster_id: str,
            broadcaster_login: str,
            event: dict,
            *,
            message_id: str | None = None,
        ) -> None:
            del broadcaster_id, broadcaster_login, event
            callback_calls.append(str(message_id))
            await release.wait()

        handler.set_callback("channel.follow", _follow_callback)
        handler.activate_notification_dispatch()
        body = {
            "subscription": {
                "type": "channel.follow",
                "condition": {
                    "broadcaster_user_id": "457002490",
                    "moderator_user_id": "999",
                },
            },
            "event": {
                "broadcaster_user_id": "457002490",
                "broadcaster_user_login": "tolgiziusx3",
                "user_id": "111",
                "user_login": "newfollower",
            },
        }

        first = await handler.dispatch_notification_internal_async(
            body,
            "channel.follow",
            message_id="msg-internal-1",
        )
        duplicate = await handler.dispatch_notification_internal_async(
            body,
            "channel.follow",
            message_id="msg-internal-1",
        )

        self.assertTrue(first.get("ok"))
        self.assertTrue(first.get("queued"))
        self.assertFalse(first.get("duplicate"))
        self.assertTrue(duplicate.get("ok"))
        self.assertFalse(duplicate.get("queued"))
        self.assertTrue(duplicate.get("duplicate"))

        await asyncio.sleep(0)
        self.assertEqual(callback_calls, ["msg-internal-1"])
        release.set()
        await asyncio.sleep(0)

    async def test_internal_dispatch_does_not_collide_with_external_replay_state(self) -> None:
        shared_state_store = EventSubStateStore(
            repository=InMemoryEventSubStateRepository(),
        )
        external_calls: list[str | None] = []
        internal_calls: list[str | None] = []

        async def _external_callback(
            broadcaster_id: str,
            broadcaster_login: str,
            event: dict,
            *,
            message_id: str | None = None,
        ) -> None:
            del broadcaster_id, broadcaster_login, event
            external_calls.append(message_id)

        async def _internal_callback(
            broadcaster_id: str,
            broadcaster_login: str,
            event: dict,
            *,
            message_id: str | None = None,
        ) -> None:
            del broadcaster_id, broadcaster_login, event
            internal_calls.append(message_id)

        external_handler = EventSubWebhookHandler(
            secret="test-secret",
            synchronous_notifications=True,
            state_store=shared_state_store,
        )
        external_handler.set_callback("stream.offline", _external_callback)
        external_handler.activate_notification_dispatch()

        internal_handler = EventSubWebhookHandler(
            secret="test-secret",
            synchronous_notifications=True,
            state_store=shared_state_store,
        )
        internal_handler.set_callback("stream.offline", _internal_callback)
        internal_handler.activate_notification_dispatch()

        body = {
            "subscription": {
                "type": "stream.offline",
                "condition": {"broadcaster_user_id": "993954638"},
            },
            "event": {
                "broadcaster_user_id": "993954638",
                "broadcaster_user_login": "denoshock",
            },
        }
        request = self._signed_request(
            secret="test-secret",
            body=body,
            message_id="msg-shared-state-offline-1",
        )

        response = await external_handler.handle_request(request)
        internal_result = await internal_handler.dispatch_notification_internal_async(
            body,
            "stream.offline",
            message_id="msg-shared-state-offline-1",
        )

        self.assertEqual(response.status, 204)
        self.assertEqual(external_calls, ["msg-shared-state-offline-1"])
        self.assertTrue(internal_result.get("ok"))
        self.assertFalse(internal_result.get("duplicate"))
        self.assertTrue(internal_result.get("processed"))
        self.assertEqual(internal_calls, ["msg-shared-state-offline-1"])

    async def test_internal_sync_dispatch_is_deprecated(self) -> None:
        handler = EventSubWebhookHandler(secret="test-secret")
        with self.assertRaisesRegex(
            RuntimeError,
            "dispatch_notification_internal is deprecated",
        ):
            handler.dispatch_notification_internal(
                {"subscription": {"type": "stream.offline"}, "event": {}},
                "stream.offline",
                message_id="msg-internal-sync-deprecated",
            )

    async def test_internal_dispatch_rejects_missing_callback_without_tracking_message_id(self) -> None:
        handler = EventSubWebhookHandler(secret="test-secret")
        handler.activate_notification_dispatch()
        loop = asyncio.get_running_loop()
        loop_errors: list[dict] = []
        previous_handler = loop.get_exception_handler()

        def _capture_exception(loop_ref, context) -> None:
            del loop_ref
            loop_errors.append(dict(context))

        loop.set_exception_handler(_capture_exception)
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

        try:
            with self.assertRaisesRegex(RuntimeError, "no callback registered"):
                await handler.dispatch_notification_internal_async(
                    body,
                    "stream.offline",
                    message_id="msg-internal-missing-callback",
                )
        finally:
            loop.set_exception_handler(previous_handler)

        self.assertEqual(loop_errors, [])
        self.assertFalse(handler._is_duplicate("msg-internal-missing-callback"))

    async def test_internal_dispatch_rejects_notification_before_activation(self) -> None:
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

        with self.assertRaisesRegex(RuntimeError, "dispatch inactive"):
            await handler.dispatch_notification_internal_async(
                body,
                "stream.offline",
                message_id="msg-internal-inactive",
            )

        self.assertFalse(handler._is_duplicate("msg-internal-inactive"))

    async def test_internal_dispatch_releases_message_id_when_queueing_fails(self) -> None:
        handler = EventSubWebhookHandler(secret="test-secret")
        handler.activate_notification_dispatch()
        handler.set_callback("channel.follow", self._async_noop)
        body = {
            "subscription": {
                "type": "channel.follow",
                "condition": {
                    "broadcaster_user_id": "457002490",
                    "moderator_user_id": "999",
                },
            },
            "event": {
                "broadcaster_user_id": "457002490",
                "broadcaster_user_login": "tolgiziusx3",
                "user_id": "111",
                "user_login": "newfollower",
            },
        }

        with patch(
            "bot.monitoring.eventsub_webhook.asyncio.create_task",
            side_effect=RuntimeError("loop unavailable"),
        ):
            with self.assertRaisesRegex(RuntimeError, "loop unavailable"):
                await handler.dispatch_notification_internal_async(
                    body,
                    "channel.follow",
                    message_id="msg-internal-queue-failure",
                )

        self.assertFalse(handler._is_duplicate("msg-internal-queue-failure"))

    async def test_internal_async_dispatch_in_sync_mode_runs_callback_inline(self) -> None:
        handler = EventSubWebhookHandler(secret="test-secret", synchronous_notifications=True)
        callback_calls: list[str | None] = []

        async def _offline_callback(
            broadcaster_id: str,
            broadcaster_login: str,
            event: dict,
            *,
            message_id: str | None = None,
        ) -> None:
            del broadcaster_id, broadcaster_login, event
            callback_calls.append(message_id)

        handler.set_callback("stream.offline", _offline_callback)
        handler.activate_notification_dispatch()
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

        result = await handler.dispatch_notification_internal_async(
            body,
            "stream.offline",
            message_id="msg-internal-sync-1",
        )

        self.assertTrue(result.get("ok"))
        self.assertFalse(result.get("queued"))
        self.assertTrue(result.get("processed"))
        self.assertEqual(callback_calls, ["msg-internal-sync-1"])

    async def test_restart_stable_replay_guard_rejects_duplicate_after_handler_recreation(self) -> None:
        state_store = EventSubStateStore(
            repository=InMemoryEventSubStateRepository(),
        )
        first_calls: list[str | None] = []

        async def _offline_callback_first(
            broadcaster_id: str,
            broadcaster_login: str,
            event: dict,
            *,
            message_id: str | None = None,
        ) -> None:
            del broadcaster_id, broadcaster_login, event
            first_calls.append(message_id)

        first_handler = EventSubWebhookHandler(
            secret="test-secret",
            synchronous_notifications=True,
            state_store=state_store,
        )
        first_handler.set_callback("stream.offline", _offline_callback_first)
        first_handler.activate_notification_dispatch()

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
            message_id="msg-persistent-replay-1",
        )

        first_response = await first_handler.handle_request(request)
        self.assertEqual(first_response.status, 204)
        self.assertEqual(first_calls, ["msg-persistent-replay-1"])

        second_calls: list[str | None] = []

        async def _offline_callback_second(
            broadcaster_id: str,
            broadcaster_login: str,
            event: dict,
            *,
            message_id: str | None = None,
        ) -> None:
            del broadcaster_id, broadcaster_login, event
            second_calls.append(message_id)

        restarted_handler = EventSubWebhookHandler(
            secret="test-secret",
            synchronous_notifications=True,
            state_store=state_store,
        )
        restarted_handler.set_callback("stream.offline", _offline_callback_second)
        restarted_handler.activate_notification_dispatch()

        replay_response = await restarted_handler.handle_request(request)

        self.assertEqual(replay_response.status, 204)
        self.assertEqual(second_calls, [])

    @staticmethod
    def _async_return(value):
        async def _reader():
            return value

        return _reader

    @staticmethod
    async def _async_noop(*_args, **_kwargs) -> None:
        return None

    @staticmethod
    def _signed_request(
        *,
        secret: str,
        body: dict,
        message_id: str,
        timestamp: str | None = None,
        message_type: str = "notification",
        subscription_type: str = "stream.offline",
    ) -> SimpleNamespace:
        raw_body = json.dumps(body).encode("utf-8")
        timestamp = timestamp or datetime.now(UTC).isoformat().replace("+00:00", "Z")
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
                "Twitch-Eventsub-Message-Type": message_type,
                "Twitch-Eventsub-Subscription-Type": subscription_type,
            },
            read=EventSubWebhookRequestValidationTests._async_return(raw_body),
        )


if __name__ == "__main__":
    unittest.main()
