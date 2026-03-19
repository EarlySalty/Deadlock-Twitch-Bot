import unittest
from unittest.mock import patch

from bot.chat.moderation import ModerationMixin


class _InviteHarness(ModerationMixin):
    def __init__(self) -> None:
        self.prefix = "!"
        self._last_invite_reply: dict[str, float] = {}
        self._last_invite_reply_user: dict[tuple[str, str], float] = {}
        self._last_promo_sent: dict[str, float] = {}
        self.sent_messages: list[str] = []

    async def _get_promo_invite(self, login: str):
        del login
        return "https://discord.gg/example", False

    async def _send_chat_message(self, channel, text: str, source: str = "") -> bool:
        del channel, source
        self.sent_messages.append(text)
        return True


class _DummyChannel:
    def __init__(self, name: str) -> None:
        self.name = name
        self.login = name


class _DummyAuthor:
    def __init__(self, name: str) -> None:
        self.name = name


class _DummyMessage:
    def __init__(self, *, content: str, channel_name: str = "partner_one", author_name: str = "viewer") -> None:
        self.content = content
        self.channel = _DummyChannel(channel_name)
        self.author = _DummyAuthor(author_name)


class DeadlockInviteDetectionTests(unittest.IsolatedAsyncioTestCase):
    def test_colloquial_where_to_play_question_is_detected(self) -> None:
        harness = _InviteHarness()

        self.assertTrue(harness._looks_like_deadlock_access_question("Wie und wo kann man das zocken?"))

    def test_gameplay_question_without_access_intent_is_ignored(self) -> None:
        harness = _InviteHarness()

        self.assertFalse(harness._looks_like_deadlock_access_question("Wie spielt man Lash?"))

    def test_can_one_join_is_detected(self) -> None:
        harness = _InviteHarness()

        self.assertTrue(harness._looks_like_deadlock_access_question("Kann man sich anschließen?"))

    def test_can_one_play_along_is_detected(self) -> None:
        harness = _InviteHarness()

        self.assertTrue(harness._looks_like_deadlock_access_question("Kann man mitspielen oder mitzocken?"))

    async def test_maybe_send_deadlock_access_hint_answers_colloquial_question(self) -> None:
        harness = _InviteHarness()
        message = _DummyMessage(content="Wie und wo kann man das zocken?")

        with patch(
            "bot.core.partner_utils.is_partner_channel_for_chat_tracking",
            return_value=True,
        ):
            ok = await harness._maybe_send_deadlock_access_hint(message)

        self.assertTrue(ok)
        self.assertEqual(len(harness.sent_messages), 1)
        self.assertIn("https://discord.gg/example", harness.sent_messages[0])


if __name__ == "__main__":
    unittest.main()
