import unittest

from bot.dashboard.live.live import _BOT_REQUIRED_SCOPES, _BOT_SCOPE_LABELS


class DashboardLiveBotScopesTests(unittest.TestCase):
    def test_announcements_scope_is_tracked_in_bot_scope_matrix(self) -> None:
        self.assertIn("moderator:manage:announcements", _BOT_REQUIRED_SCOPES)
        self.assertEqual(
            _BOT_SCOPE_LABELS["moderator:manage:announcements"],
            "Bot Announcements",
        )
