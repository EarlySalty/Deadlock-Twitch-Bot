from __future__ import annotations

import unittest
from unittest.mock import Mock

from bot.raid.chat_targets import (
    ChatTarget,
    lookup_outbound_chat_suppression,
    make_chat_target,
    normalize_chat_target_login,
)


class RaidChatTargetsTests(unittest.TestCase):
    def test_make_chat_target_returns_stable_shape(self) -> None:
        target = make_chat_target("  RealClassik  ", " 471205134 ")

        self.assertIsInstance(target, ChatTarget)
        self.assertEqual(target.name, "realclassik")
        self.assertEqual(target.id, "471205134")

    def test_lookup_outbound_chat_suppression_returns_suppression_when_available(self) -> None:
        chat_bot = Mock()
        chat_bot._get_outbound_chat_suppression.return_value = {"reason_code": "channel_settings"}

        suppression = lookup_outbound_chat_suppression(
            chat_bot,
            target_login="RealClassik",
            target_id=" 471205134 ",
            source="recruitment",
        )

        self.assertEqual(suppression, {"reason_code": "channel_settings"})
        chat_bot._get_outbound_chat_suppression.assert_called_once()
        channel, source = chat_bot._get_outbound_chat_suppression.call_args.args
        self.assertEqual(channel.name, "realclassik")
        self.assertEqual(channel.id, "471205134")
        self.assertEqual(source, "recruitment")

    def test_lookup_outbound_chat_suppression_returns_none_without_target_id(self) -> None:
        chat_bot = Mock()

        suppression = lookup_outbound_chat_suppression(
            chat_bot,
            target_login="RealClassik",
            target_id="",
            source="recruitment",
        )

        self.assertIsNone(suppression)
        chat_bot._get_outbound_chat_suppression.assert_not_called()

    def test_lookup_outbound_chat_suppression_returns_none_on_errors(self) -> None:
        chat_bot = Mock()
        chat_bot._get_outbound_chat_suppression.side_effect = RuntimeError("boom")

        suppression = lookup_outbound_chat_suppression(
            chat_bot,
            target_login="RealClassik",
            target_id="471205134",
            source="recruitment",
        )

        self.assertIsNone(suppression)

    def test_normalize_chat_target_login_lowercases(self) -> None:
        self.assertEqual(normalize_chat_target_login("  ReAl  "), "real")


if __name__ == "__main__":
    unittest.main()
