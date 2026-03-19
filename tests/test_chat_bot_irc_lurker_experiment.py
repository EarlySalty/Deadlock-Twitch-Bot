import unittest
from types import SimpleNamespace
from unittest.mock import patch

from bot.base import TwitchBaseCog


class _FakeIRCLurkerTracker:
    def __init__(self, client_id: str, access_token: str, *, nick: str | None = None) -> None:
        self.client_id = client_id
        self.access_token = access_token
        self.nick = nick
        self.running = False
        self.channels: set[str] = set()
        self.track_calls: list[tuple[str, str]] = []
        self.untrack_calls: list[str] = []

    async def start(self) -> None:
        self.running = True

    async def stop(self) -> None:
        self.running = False

    async def track_channel(self, channel: str, *, mode: str = "partner") -> None:
        self.channels.add(channel)
        self.track_calls.append((channel, mode))

    async def untrack_channel(self, channel: str) -> None:
        self.channels.discard(channel)
        self.untrack_calls.append(channel)


class _IRCLurkerHarness(TwitchBaseCog):
    def __init__(self) -> None:
        pass

    def _log_chat_bot_lifecycle_event(self, **payload) -> None:
        self.logged_events.append(payload)


class ChatBotIRCLurkerExperimentTests(unittest.IsolatedAsyncioTestCase):
    async def test_experimental_tracker_starts_and_syncs_chat_runtime_channels(self) -> None:
        harness = _IRCLurkerHarness()
        harness._experimental_irc_lurker_enabled = True
        harness._experimental_irc_lurker_channels = {"partner_one", "category_one"}
        harness._irc_lurker_tracker = None
        harness._twitch_bot_client_id = "client-id"
        harness._bot_token_manager = SimpleNamespace(
            access_token="oauth:test-token",
            bot_login="deadbot",
            scopes={"user:read:chat", "moderator:read:chatters"},
        )
        harness._twitch_chat_bot = SimpleNamespace(
            _monitored_streamers={"partner_one"},
            _channel_subscription_types={"category_one": {"channel.chat.message"}},
            _channel_ids={"partner_one": "1001"},
            _is_partner_channel_for_chat_tracking=lambda login: login == "partner_one",
        )
        harness.logged_events = []

        with patch("bot.base.IRCLurkerTracker", _FakeIRCLurkerTracker):
            started = await harness._ensure_experimental_irc_lurker_tracker_started()
            sync = await harness._sync_experimental_irc_lurker_tracker_channels()

        self.assertTrue(started)
        self.assertIsNotNone(harness._irc_lurker_tracker)
        self.assertEqual(harness._irc_lurker_tracker.nick, "deadbot")
        self.assertEqual(
            harness._irc_lurker_tracker.track_calls,
            [("category_one", "category"), ("partner_one", "partner")],
        )
        self.assertEqual(sync["tracked"], 2)
        self.assertIn("irc_lurker_experiment_started", [e["event"] for e in harness.logged_events])

    async def test_experimental_tracker_is_skipped_without_bot_login(self) -> None:
        harness = _IRCLurkerHarness()
        harness._experimental_irc_lurker_enabled = True
        harness._experimental_irc_lurker_channels = {"earlysalty"}
        harness._irc_lurker_tracker = None
        harness._twitch_bot_client_id = "client-id"
        harness._bot_token_manager = SimpleNamespace(
            access_token="oauth:test-token",
            bot_login=None,
            scopes={"user:read:chat"},
        )
        harness.logged_events = []

        with patch("bot.base.IRCLurkerTracker", _FakeIRCLurkerTracker):
            started = await harness._ensure_experimental_irc_lurker_tracker_started()

        self.assertFalse(started)
        self.assertIsNone(harness._irc_lurker_tracker)
        self.assertIn("irc_lurker_experiment_skipped", [e["event"] for e in harness.logged_events])

    async def test_sync_experimental_tracker_filters_to_explicit_allowlist(self) -> None:
        harness = _IRCLurkerHarness()
        harness._experimental_irc_lurker_enabled = True
        harness._experimental_irc_lurker_channels = {"earlysalty"}
        harness._irc_lurker_tracker = _FakeIRCLurkerTracker("client-id", "oauth:test-token", nick="deadbot")
        harness._twitch_chat_bot = SimpleNamespace(
            _monitored_streamers={"partner_one"},
            _channel_subscription_types={"category_one": {"channel.chat.message"}},
            _channel_ids={},
            _is_partner_channel_for_chat_tracking=lambda login: True,
        )
        harness.logged_events = []

        sync = await harness._sync_experimental_irc_lurker_tracker_channels()

        self.assertEqual(harness._irc_lurker_tracker.track_calls, [("earlysalty", "partner")])
        self.assertEqual(sync["tracked"], 1)
        self.assertIn("forced_allowlist", sync["runtime_sources"])


if __name__ == "__main__":
    unittest.main()
