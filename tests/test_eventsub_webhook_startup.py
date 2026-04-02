import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock

from bot.monitoring.eventsub_mixin import _EventSubMixin


class _FakeWebhookHandler:
    """Accepts set_callback calls without doing anything."""

    def set_callback(self, sub_type: str, callback) -> None:
        pass


class _WebhookStartupHarness(_EventSubMixin):
    def __init__(self) -> None:
        self._webhook_secret = "test-secret"
        self._eventsub_webhook_handler = _FakeWebhookHandler()
        self._eventsub_started = False
        self._eventsub_webhook_active_subs = []
        self._eventsub_webhook_tracked = set()
        self.api = SimpleNamespace(
            get_streams_by_logins=AsyncMock(return_value=[]),
        )
        self.bot = SimpleNamespace(wait_until_ready=AsyncMock())

    def _get_eventsub_webhook_url(self) -> str:
        return "https://earlysalty.de/twitch/eventsub/callback"

    def _get_raid_enabled_streamers_for_eventsub(self) -> list[dict]:
        return [{"twitch_user_id": "123", "twitch_login": "partner_one"}]

    def _set_eventsub_webhook_notification_dispatch(self, *, active: bool) -> None:
        return None

    async def _ensure_eventsub_processing_inbox_started(self) -> None:
        return None

    async def _record_eventsub_capacity_snapshot(self, reason: str, *, force: bool = False) -> None:
        return None


class EventSubWebhookStartupTests(unittest.IsolatedAsyncioTestCase):
    async def _run_startup(
        self,
        harness: _WebhookStartupHarness,
        *,
        channel_raid_sub_succeeds: bool = True,
    ) -> tuple[bool, list[dict]]:
        """Run _start_eventsub_listener with mocked sub creation and healthy startup."""
        subscription_calls: list[dict] = []

        async def _fake_create_sub(
            *, sub_type: str, condition: dict, webhook_url: str, secret: str, oauth_token, **_
        ) -> tuple[bool, bool]:
            subscription_calls.append({"sub_type": sub_type, "condition": dict(condition)})
            if sub_type == "channel.raid" and not channel_raid_sub_succeeds:
                return False, False
            return True, False

        harness._create_eventsub_webhook_subscription = _fake_create_sub  # type: ignore[method-assign]
        harness._is_eventsub_webhook_startup_healthy = lambda *a, **kw: (True, [])  # type: ignore[method-assign]

        started = await harness._start_eventsub_listener()
        return started, subscription_calls

    async def test_webhook_startup_subscribes_channel_raid_with_to_broadcaster_user_id(self) -> None:
        """
        The Webhook startup loop must use condition={"to_broadcaster_user_id": bid}
        for channel.raid — NOT {"broadcaster_user_id": bid}.

        A wrong condition key causes Twitch to silently ignore the subscription:
        raid events are never delivered and the bot misses incoming raids entirely.
        """
        harness = _WebhookStartupHarness()
        started, calls = await self._run_startup(harness)

        self.assertTrue(started)
        raid_calls = [c for c in calls if c["sub_type"] == "channel.raid"]
        self.assertEqual(len(raid_calls), 1)
        self.assertEqual(raid_calls[0]["condition"], {"to_broadcaster_user_id": "123"})
        self.assertNotIn("broadcaster_user_id", raid_calls[0]["condition"])
        self.assertIn(("channel.raid", "123"), harness._eventsub_webhook_tracked)

    async def test_webhook_startup_does_not_track_channel_raid_when_subscription_fails(
        self,
    ) -> None:
        """
        If _create_eventsub_webhook_subscription returns (False, ...) for channel.raid,
        _eventsub_track_sub must NOT be called — otherwise ensure_raid_target_dynamic_ready
        would see local_tracking=True and skip the dynamic fallback, even though no
        actual subscription exists at Twitch.
        """
        harness = _WebhookStartupHarness()
        _, _ = await self._run_startup(harness, channel_raid_sub_succeeds=False)

        self.assertNotIn(("channel.raid", "123"), harness._eventsub_webhook_tracked)

    async def test_webhook_startup_subscribes_all_four_core_types(self) -> None:
        """
        Startup must cover all four core subscription types per streamer:
        stream.online, stream.offline, channel.update, channel.raid.
        Missing any of them leaves a monitoring blind spot.
        """
        harness = _WebhookStartupHarness()
        started, calls = await self._run_startup(harness)

        self.assertTrue(started)
        subscribed_types = {c["sub_type"] for c in calls}
        self.assertIn("stream.online", subscribed_types)
        self.assertIn("stream.offline", subscribed_types)
        self.assertIn("channel.update", subscribed_types)
        self.assertIn("channel.raid", subscribed_types)


if __name__ == "__main__":
    unittest.main()
