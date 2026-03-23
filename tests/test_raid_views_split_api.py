import os
import unittest
from unittest.mock import patch

from bot.raid import views


class RaidViewsSplitApiTests(unittest.TestCase):
    def test_internal_api_auth_url_requires_base_url_and_token(self) -> None:
        with patch.dict(os.environ, {}, clear=True):
            self.assertIsNone(views._split_internal_api_auth_url(12345))

    def test_internal_api_auth_url_normalizes_internal_path(self) -> None:
        with patch.dict(
            os.environ,
            {
                "TWITCH_INTERNAL_API_BASE_URL": "http://127.0.0.1:8776",
                "TWITCH_INTERNAL_API_TOKEN": "secret-token",
            },
            clear=True,
        ):
            result = views._split_internal_api_auth_url(12345)

        self.assertIsNotNone(result)
        assert result is not None
        url, headers = result
        self.assertEqual(
            url,
            "http://127.0.0.1:8776/internal/twitch/v1/raid/auth-url?login=discord%3A12345&discord_user_id=12345",
        )
        self.assertEqual(headers, {"X-Internal-Token": "secret-token"})

    def test_prefer_split_internal_api_disabled_for_enforced_bot_role(self) -> None:
        with patch.dict(
            os.environ,
            {
                "TWITCH_INTERNAL_API_BASE_URL": "http://127.0.0.1:8776",
                "TWITCH_INTERNAL_API_TOKEN": "secret-token",
                "TWITCH_SPLIT_RUNTIME_ROLE": "bot",
                "TWITCH_SPLIT_RUNTIME_ENFORCE": "1",
            },
            clear=True,
        ):
            self.assertFalse(views._prefer_split_internal_raid_auth_api())

    def test_prefer_split_internal_api_enabled_when_configured(self) -> None:
        with patch.dict(
            os.environ,
            {
                "TWITCH_INTERNAL_API_BASE_URL": "http://127.0.0.1:8776",
                "TWITCH_INTERNAL_API_TOKEN": "secret-token",
            },
            clear=True,
        ):
            self.assertTrue(views._prefer_split_internal_raid_auth_api())


if __name__ == "__main__":
    unittest.main()
