import asyncio
import unittest
from types import SimpleNamespace
from unittest import mock

from bot.engagement import irc_reader


class ParseTests(unittest.TestCase):
    def test_parse_tags(self):
        tags = irc_reader._parse_tags("display-name=Foo;room-id=123;id=abc;user-id=456")
        self.assertEqual(tags["room-id"], "123")
        self.assertEqual(tags["user-id"], "456")
        self.assertEqual(tags["id"], "abc")

    def test_privmsg_regex(self):
        m = irc_reader._PRIVMSG_RE.match(
            ":foo!foo@foo.tmi.twitch.tv PRIVMSG #solidoz :hallo welt :-)"
        )
        self.assertIsNotNone(m)
        self.assertEqual(m.group("login"), "foo")
        self.assertEqual(m.group("channel"), "solidoz")
        self.assertEqual(m.group("text"), "hallo welt :-)")


class ProcessTests(unittest.TestCase):
    def _line(self, login="viewer1", channel="solidoz", text="welcher build für pocket?"):
        return (
            f"@display-name={login};id=msg-1;room-id=999;user-id=42 "
            f":{login}!{login}@{login}.tmi.twitch.tv PRIVMSG #{channel} :{text}"
        )

    def test_spoke_routes_to_stealth_send_with_room_id(self):
        reader = irc_reader.EngagementIrcReader()
        fake_pipeline = mock.Mock()
        fake_pipeline.handle = mock.AsyncMock(
            return_value=SimpleNamespace(response_text="Pocket-Build: Spirit-heavy.")
        )
        sent = {}

        async def fake_send(room_id, text):
            sent["room_id"] = room_id
            sent["text"] = text
            return True

        with mock.patch.object(irc_reader, "get_pipeline", return_value=fake_pipeline), \
             mock.patch.object(irc_reader, "_stealth_send", new=fake_send):
            asyncio.run(reader._handle_line(self._line()))

        # Pipeline bekam die geparste Nachricht
        call = fake_pipeline.handle.call_args.args[0]
        self.assertEqual(call.channel_login, "solidoz")
        self.assertEqual(call.twitch_user_id, "42")
        self.assertEqual(call.twitch_login, "viewer1")
        self.assertEqual(call.content, "welcher build für pocket?")
        # Antwort ging via Stealth-Send an die room-id (broadcaster)
        self.assertEqual(sent["room_id"], "999")
        self.assertEqual(sent["text"], "Pocket-Build: Spirit-heavy.")

    def test_self_message_is_skipped(self):
        reader = irc_reader.EngagementIrcReader()
        fake_pipeline = mock.Mock()
        fake_pipeline.handle = mock.AsyncMock()
        with mock.patch.object(irc_reader, "get_pipeline", return_value=fake_pipeline):
            asyncio.run(reader._handle_line(self._line(login=irc_reader.sender_auth.SENDER_LOGIN)))
        fake_pipeline.handle.assert_not_called()

    def test_known_bot_is_skipped(self):
        reader = irc_reader.EngagementIrcReader()
        fake_pipeline = mock.Mock()
        fake_pipeline.handle = mock.AsyncMock()
        with mock.patch.object(irc_reader, "get_pipeline", return_value=fake_pipeline), \
             mock.patch.object(irc_reader, "is_known_chat_bot", return_value=True):
            asyncio.run(reader._handle_line(self._line(login="nightbot")))
        fake_pipeline.handle.assert_not_called()

    def test_silent_response_no_send(self):
        reader = irc_reader.EngagementIrcReader()
        fake_pipeline = mock.Mock()
        fake_pipeline.handle = mock.AsyncMock(
            return_value=SimpleNamespace(response_text=None)
        )
        called = {"sent": False}

        async def fake_send(room_id, text):
            called["sent"] = True
            return True

        with mock.patch.object(irc_reader, "get_pipeline", return_value=fake_pipeline), \
             mock.patch.object(irc_reader, "_stealth_send", new=fake_send):
            asyncio.run(reader._handle_line(self._line()))
        self.assertFalse(called["sent"])

    def test_ping_is_answered(self):
        reader = irc_reader.EngagementIrcReader()
        writes = []
        reader.writer = SimpleNamespace(
            write=lambda b: writes.append(b),
            drain=mock.AsyncMock(),
        )
        asyncio.run(reader._handle_line("PING :tmi.twitch.tv"))
        self.assertTrue(any(b"PONG" in w for w in writes))


if __name__ == "__main__":
    unittest.main()
