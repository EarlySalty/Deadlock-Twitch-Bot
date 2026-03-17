import unittest

from bot.internal_api import INTERNAL_API_BASE_PATH
from bot.internal_api.client import InternalApiClient


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
    def __init__(self, *, response: _FakeResponse) -> None:
        self._response = response
        self.closed = False
        self.calls: list[dict] = []

    async def request(self, method: str, url: str, **kwargs):
        self.calls.append({"method": method, "url": url, "kwargs": kwargs})
        return self._response

    async def close(self) -> None:
        self.closed = True


class InternalApiClientTests(unittest.IsolatedAsyncioTestCase):
    async def test_get_observability_snapshot_calls_internal_endpoint(self) -> None:
        session = _FakeSession(
            response=_FakeResponse(
                status=200,
                text='{"ok":true,"observability":{"chat":{"joinAttempts":3}}}',
            )
        )
        client = InternalApiClient(
            base_url="http://127.0.0.1:8776",
            token="secret",
            session=session,
        )

        payload = await client.get_observability_snapshot()

        self.assertTrue(payload["ok"])
        self.assertEqual(payload["observability"]["chat"]["joinAttempts"], 3)
        self.assertEqual(
            session.calls[0]["url"],
            "http://127.0.0.1:8776/internal/twitch/v1/debug/observability",
        )

    async def test_get_active_live_announcements_calls_internal_endpoint(self) -> None:
        session = _FakeSession(
            response=_FakeResponse(
                status=200,
                text=(
                    '[{"streamer_login":"partner_one","message_id":123,'
                    '"tracking_token":"deadbeef1234",'
                    '"referral_url":"https://www.twitch.tv/partner_one?ref=DE-Deadlock-Discord",'
                    '"button_label":"Auf Twitch ansehen","channel_id":456}]'
                ),
            )
        )
        client = InternalApiClient(
            base_url="http://127.0.0.1:8776",
            token="secret",
            session=session,
        )

        payload = await client.get_active_live_announcements()

        self.assertEqual(len(payload), 1)
        self.assertEqual(payload[0]["streamer_login"], "partner_one")
        self.assertEqual(
            session.calls[0]["url"],
            "http://127.0.0.1:8776/internal/twitch/v1/live/active-announcements",
        )

    async def test_record_live_link_click_sends_expected_payload_and_idempotency_header(self) -> None:
        session = _FakeSession(response=_FakeResponse(status=200, text='{"ok":true}'))
        client = InternalApiClient(
            base_url=f"http://127.0.0.1:8776{INTERNAL_API_BASE_PATH}",
            token="secret",
            session=session,
        )

        payload = await client.record_live_link_click(
            streamer_login="Partner_One",
            tracking_token="deadbeef1234",
            discord_user_id="12345",
            discord_username="Viewer One",
            guild_id="111",
            channel_id="222",
            message_id="333",
            source_hint="discord_button",
            idempotency_key="live-click-1",
        )

        self.assertEqual(payload, {"ok": True})
        self.assertEqual(len(session.calls), 1)
        self.assertEqual(
            session.calls[0]["url"],
            "http://127.0.0.1:8776/internal/twitch/v1/live/link-click",
        )
        self.assertEqual(
            session.calls[0]["kwargs"]["headers"]["Idempotency-Key"],
            "live-click-1",
        )
        self.assertEqual(
            session.calls[0]["kwargs"]["json"],
            {
                "streamer_login": "partner_one",
                "tracking_token": "deadbeef1234",
                "discord_user_id": "12345",
                "discord_username": "Viewer One",
                "guild_id": "111",
                "channel_id": "222",
                "message_id": "333",
                "source_hint": "discord_button",
            },
        )


if __name__ == "__main__":
    unittest.main()
