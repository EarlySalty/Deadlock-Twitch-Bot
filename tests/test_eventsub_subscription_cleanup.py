import unittest

from bot.monitoring.eventsub_mixin import _EventSubMixin


class _CleanupHarness(_EventSubMixin):
    def __init__(self) -> None:
        self.api = _FakeApi()


class _FakeApi:
    def __init__(self) -> None:
        self.list_statuses: list[str] = []
        self.deleted_ids: list[str] = []

    async def list_eventsub_subscriptions(self, *, status: str):
        self.list_statuses.append(status)
        return [
            {
                "id": "sub-enabled",
                "status": "enabled",
                "transport": {"callback": "https://example.com/twitch/eventsub/callback"},
            },
            {
                "id": "sub-pending",
                "status": "webhook_callback_verification_pending",
                "transport": {"callback": "https://example.com/twitch/eventsub/callback"},
            },
            {
                "id": "sub-other",
                "status": "enabled",
                "transport": {"callback": "https://other.example.com/twitch/eventsub/callback"},
            },
        ]

    async def delete_eventsub_subscription(self, sub_id: str) -> None:
        self.deleted_ids.append(sub_id)


class EventSubSubscriptionCleanupTests(unittest.IsolatedAsyncioTestCase):
    async def test_cleanup_old_eventsub_subscriptions_includes_non_enabled_states(self) -> None:
        harness = _CleanupHarness()

        await harness._cleanup_old_eventsub_subscriptions(
            "https://example.com/twitch/eventsub/callback"
        )

        self.assertEqual(harness.api.list_statuses, [""])
        self.assertEqual(harness.api.deleted_ids, ["sub-enabled", "sub-pending"])

    async def test_cleanup_old_eventsub_subscriptions_preserves_active_targets(self) -> None:
        harness = _CleanupHarness()

        async def _list_with_conditions(*, status: str):
            harness.api.list_statuses.append(status)
            return [
                {
                    "id": "sub-active",
                    "status": "enabled",
                    "condition": {"broadcaster_user_id": "123"},
                    "transport": {"callback": "https://example.com/twitch/eventsub/callback"},
                },
                {
                    "id": "sub-stale",
                    "status": "enabled",
                    "condition": {"broadcaster_user_id": "999"},
                    "transport": {"callback": "https://example.com/twitch/eventsub/callback"},
                },
            ]

        harness.api.list_eventsub_subscriptions = _list_with_conditions

        await harness._cleanup_old_eventsub_subscriptions(
            "https://example.com/twitch/eventsub/callback",
            active_target_user_ids={"123"},
        )

        self.assertEqual(harness.api.deleted_ids, ["sub-stale"])


if __name__ == "__main__":
    unittest.main()
