import unittest
from unittest.mock import AsyncMock, patch

from bot.chat.irc_lurker_tracker import IRCLurkerTracker


class _FakeReader:
    def __init__(self, lines: list[bytes]) -> None:
        self._lines = list(lines)

    async def readline(self) -> bytes:
        if self._lines:
            return self._lines.pop(0)
        return b""


class _FakeWriter:
    def __init__(self) -> None:
        self.writes: list[bytes] = []
        self.closed = False

    def write(self, data: bytes) -> None:
        self.writes.append(data)

    async def drain(self) -> None:
        return None

    def close(self) -> None:
        self.closed = True

    async def wait_closed(self) -> None:
        return None


class IRCLurkerTrackerTests(unittest.IsolatedAsyncioTestCase):
    async def test_connect_uses_authenticated_bot_nick_when_available(self) -> None:
        tracker = IRCLurkerTracker("client-id", "oauth:secret-token", nick="DeadBot")
        reader = _FakeReader([b":tmi.twitch.tv 001 deadbot :Welcome\r\n"])
        writer = _FakeWriter()

        with patch(
            "bot.chat.irc_lurker_tracker.asyncio.open_connection",
            AsyncMock(return_value=(reader, writer)),
        ):
            ok = await tracker._connect()

        self.assertTrue(ok)
        self.assertTrue(tracker.connected)
        self.assertIn(b"PASS oauth:secret-token\r\n", writer.writes)
        self.assertIn(b"NICK deadbot\r\n", writer.writes)

    async def test_track_channel_registers_channel_without_attribute_error(self) -> None:
        tracker = IRCLurkerTracker("client-id", "access-token")

        await tracker.track_channel("Partner_One")

        self.assertEqual(tracker.channels, {"partner_one"})

    async def test_track_channel_joins_immediately_when_connected(self) -> None:
        tracker = IRCLurkerTracker("client-id", "access-token")
        tracker.connected = True
        tracker._join_channel = AsyncMock(return_value=True)

        await tracker.track_channel("partner_two")

        tracker._join_channel.assert_awaited_once_with("partner_two")
        self.assertEqual(tracker.channels, {"partner_two"})
        self.assertEqual(tracker.partner_channels, {"partner_two"})

    async def test_untrack_channel_removes_channel_from_runtime_set(self) -> None:
        tracker = IRCLurkerTracker("client-id", "access-token")
        tracker.channels.add("partner_three")

        await tracker.untrack_channel("partner_three")

        self.assertEqual(tracker.channels, set())

    async def test_untrack_channel_clears_stale_channel_state(self) -> None:
        tracker = IRCLurkerTracker("client-id", "access-token")
        tracker.channels.add("partner_four")
        tracker.partner_channels.add("partner_four")
        tracker.category_channels.add("partner_four")
        tracker.channel_chatters["partner_four"] = {"viewer_a", "viewer_b"}

        await tracker.untrack_channel("partner_four")

        self.assertEqual(tracker.partner_channels, set())
        self.assertEqual(tracker.category_channels, set())
        self.assertEqual(tracker.get_chatters("partner_four"), set())

    async def test_track_channel_can_register_category_mode_and_still_collect_chatters(self) -> None:
        tracker = IRCLurkerTracker("client-id", "access-token")
        tracker._update_chatter_seen = AsyncMock()

        await tracker.track_channel("category_one", mode="category")
        await tracker._on_user_join("category_one", "ViewerA")
        await tracker._on_names_list("category_one", ["ViewerA", "ViewerB"])

        self.assertEqual(tracker.channels, {"category_one"})
        self.assertEqual(tracker.category_channels, {"category_one"})
        self.assertEqual(tracker.partner_channels, set())
        self.assertEqual(tracker.get_chatters("category_one"), {"viewera", "viewerb"})
        tracker._update_chatter_seen.assert_awaited_once_with("category_one", "viewera")

    async def test_track_channel_mode_switch_replaces_previous_mode_membership(self) -> None:
        tracker = IRCLurkerTracker("client-id", "access-token")
        tracker.channel_chatters["switcher"] = {"stale_viewer"}

        await tracker.track_channel("switcher", mode="partner")
        await tracker.track_channel("switcher", mode="category")

        self.assertEqual(tracker.channels, {"switcher"})
        self.assertEqual(tracker.partner_channels, set())
        self.assertEqual(tracker.category_channels, {"switcher"})
        self.assertEqual(tracker.get_chatters("switcher"), set())

    async def test_snapshot_marks_tracker_as_experimental_secondary_source(self) -> None:
        tracker = IRCLurkerTracker("client-id", "access-token", nick="deadbot")
        tracker.running = True
        tracker.connected = True
        tracker.channels.update({"partner_one", "category_one"})

        snapshot = tracker.get_observability_snapshot()

        self.assertTrue(snapshot["experimental"])
        self.assertTrue(snapshot["authenticated"])
        self.assertEqual(snapshot["nick"], "deadbot")
        self.assertEqual(snapshot["trackedChannelCount"], 2)


if __name__ == "__main__":
    unittest.main()
