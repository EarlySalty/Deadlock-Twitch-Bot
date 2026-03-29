import unittest

from bot.analytics.api_admin import _normalize_login as admin_normalize_login
from bot.base import TwitchBaseCog
from bot.core.twitch_login import normalize_twitch_login
from bot.dashboard.server_v2 import DashboardV2Server
from bot.dashboard.affiliate.affiliate_pii import AffiliatePII
from bot.dashboard.affiliate.gutschrift import AffiliateGutschriftService
from bot.internal_api.app import InternalApiServer
from bot.raid.integration_state import _normalize_login as raid_state_normalize_login
from bot.raid.observability import RaidObservabilityService
from bot.raid.services.followers import _normalize_login as followers_normalize_login
from bot.storage.partner_registry import _normalize_login as partner_registry_normalize_login


class TwitchLoginNormalizationTests(unittest.TestCase):
    def test_shared_normalizer_accepts_supported_forms(self) -> None:
        cases = {
            "EarlySalty": "earlysalty",
            "@EarlySalty": "earlysalty",
            "https://www.twitch.tv/EarlySalty?ref=1": "earlysalty",
            "twitch.tv/EarlySalty": "earlysalty",
        }

        for raw_value, expected in cases.items():
            with self.subTest(raw_value=raw_value):
                self.assertEqual(normalize_twitch_login(raw_value), expected)
                self.assertEqual(admin_normalize_login(raw_value), expected)
                self.assertEqual(DashboardV2Server._normalize_login(raw_value), expected)
                self.assertEqual(InternalApiServer._normalize_login(raw_value), expected)
                self.assertEqual(TwitchBaseCog._normalize_login(raw_value), expected)
                self.assertEqual(partner_registry_normalize_login(raw_value), expected)
                self.assertEqual(raid_state_normalize_login(raw_value), expected)
                self.assertEqual(RaidObservabilityService._normalize_login(raw_value), expected)
                self.assertEqual(followers_normalize_login(raw_value), expected)
                self.assertEqual(AffiliatePII._normalize_login(raw_value), expected)
                self.assertEqual(AffiliateGutschriftService._normalize_login(raw_value), expected)

    def test_shared_normalizer_rejects_invalid_logins_consistently(self) -> None:
        cases = (
            "",
            "foo!!!",
            "ab",
            "https://www.twitch.tv/",
            "https://example.com/not-twitch",
            "https://www.twitch.tv/videos/123",
        )

        for raw_value in cases:
            with self.subTest(raw_value=raw_value):
                self.assertIsNone(normalize_twitch_login(raw_value))
                self.assertIsNone(admin_normalize_login(raw_value))
                self.assertIsNone(DashboardV2Server._normalize_login(raw_value))
                self.assertIsNone(InternalApiServer._normalize_login(raw_value))
                self.assertEqual(TwitchBaseCog._normalize_login(raw_value), "")
                self.assertEqual(partner_registry_normalize_login(raw_value), "")
                self.assertIsNone(raid_state_normalize_login(raw_value))
                self.assertEqual(RaidObservabilityService._normalize_login(raw_value), "")
                self.assertEqual(followers_normalize_login(raw_value), "")
                with self.assertRaises(ValueError):
                    AffiliatePII._normalize_login(raw_value)
                with self.assertRaises(ValueError):
                    AffiliateGutschriftService._normalize_login(raw_value)


if __name__ == "__main__":
    unittest.main()
