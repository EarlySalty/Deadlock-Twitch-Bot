import unittest

from bot.monitoring.eventsub_ws import EventSubTransportSessionInvalid, EventSubWSListener


class _FakeApi:
    def __init__(self, *, exc: Exception | None = None) -> None:
        self.exc = exc
        self.calls: list[dict[str, object]] = []

    async def subscribe_eventsub_websocket(
        self,
        *,
        session_id: str,
        sub_type: str,
        condition: dict,
        oauth_token: str,
    ) -> None:
        self.calls.append(
            {
                "session_id": session_id,
                "sub_type": sub_type,
                "condition": condition,
                "oauth_token": oauth_token,
            }
        )
        if self.exc is not None:
            raise self.exc


class EventSubWSListenerTests(unittest.IsolatedAsyncioTestCase):
    async def test_tracked_subscriptions_only_include_registered_entries(self) -> None:
        listener = EventSubWSListener(api=_FakeApi())
        listener.add_subscription("stream.online", "123")
        listener._token_resolver = self._resolve_token  # type: ignore[assignment]

        self.assertEqual(listener.get_tracked_subscriptions(), [])

        await listener._register_all_subscriptions("session-1")

        self.assertEqual(
            listener.get_tracked_subscriptions(),
            [
                {
                    "type": "stream.online",
                    "broadcaster_id": "123",
                    "condition": {"broadcaster_user_id": "123"},
                }
            ],
        )

    async def test_register_all_subscriptions_raises_when_any_registration_fails(self) -> None:
        listener = EventSubWSListener(api=_FakeApi(exc=RuntimeError("boom")))
        listener.add_subscription("stream.online", "123")
        listener._token_resolver = self._resolve_token  # type: ignore[assignment]

        with self.assertRaisesRegex(RuntimeError, "subscriptions failed during registration"):
            await listener._register_all_subscriptions("session-1")

    async def test_register_all_subscriptions_fails_closed_when_token_is_missing(self) -> None:
        listener = EventSubWSListener(api=_FakeApi())
        listener.add_subscription("stream.online", "123")
        listener._token_resolver = self._resolve_missing_token  # type: ignore[assignment]

        with self.assertRaisesRegex(RuntimeError, "No user token available"):
            await listener._register_all_subscriptions("session-1")

        self.assertTrue(listener.is_failed)
        self.assertFalse(listener.initial_registration_complete)

    async def test_handle_message_passes_message_id_to_callbacks_and_deduplicates(self) -> None:
        listener = EventSubWSListener(api=_FakeApi())
        callback_calls: list[str | None] = []

        async def _callback(
            broadcaster_id: str,
            broadcaster_login: str,
            event: dict,
            *,
            message_id: str | None = None,
        ) -> None:
            del broadcaster_id, broadcaster_login, event
            callback_calls.append(message_id)

        listener.set_callback("stream.offline", _callback)

        payload = {
            "metadata": {
                "message_type": "notification",
                "message_id": "ws-msg-1",
            },
            "payload": {
                "subscription": {
                    "type": "stream.offline",
                    "condition": {"broadcaster_user_id": "123"},
                },
                "event": {
                    "broadcaster_user_id": "123",
                    "broadcaster_user_login": "partner_one",
                },
            },
        }

        await listener._handle_message(payload)
        await listener._handle_message(payload)

        self.assertEqual(callback_calls, ["ws-msg-1"])

    async def test_handle_message_fails_transport_closed_on_callback_error(self) -> None:
        listener = EventSubWSListener(api=_FakeApi())

        async def _callback(
            broadcaster_id: str,
            broadcaster_login: str,
            event: dict,
            *,
            message_id: str | None = None,
        ) -> None:
            del broadcaster_id, broadcaster_login, event, message_id
            raise RuntimeError("db write failed")

        listener.set_callback("stream.offline", _callback)
        payload = {
            "metadata": {
                "message_type": "notification",
                "message_id": "ws-msg-fail-1",
            },
            "payload": {
                "subscription": {
                    "type": "stream.offline",
                    "condition": {"broadcaster_user_id": "123"},
                },
                "event": {
                    "broadcaster_user_id": "123",
                    "broadcaster_user_login": "partner_one",
                },
            },
        }

        with self.assertRaises(EventSubTransportSessionInvalid):
            await listener._handle_message(payload)

        self.assertTrue(listener.is_failed)
        self.assertFalse(listener.initial_registration_complete)
        self.assertFalse(listener._is_duplicate_message_id("ws-msg-fail-1"))

    async def test_run_bubbles_transport_failure_instead_of_reconnecting_internally(self) -> None:
        listener = EventSubWSListener(api=_FakeApi())
        attempts: list[bool] = []

        async def _run_once(*, is_reconnect: bool = False) -> None:
            attempts.append(is_reconnect)
            raise EventSubTransportSessionInvalid("callback failed")

        listener._run_once = _run_once  # type: ignore[method-assign]

        with self.assertRaisesRegex(EventSubTransportSessionInvalid, "callback failed"):
            await listener.run()

        self.assertEqual(attempts, [False])

    @staticmethod
    async def _resolve_token(_: str) -> str:
        return "oauth:test-token"

    @staticmethod
    async def _resolve_missing_token(_: str) -> str | None:
        return None


if __name__ == "__main__":
    unittest.main()
