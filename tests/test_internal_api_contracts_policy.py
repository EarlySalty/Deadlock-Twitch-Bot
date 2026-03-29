from __future__ import annotations

import datetime as dt
import ipaddress
import unittest
from uuid import UUID

from bot.internal_api.contracts import InternalApiCallbacks, IdempotencyInFlight
from bot.internal_api.policy import (
    coerce_optional_positive_int,
    compare_internal_token,
    host_without_port,
    is_loopback_host,
    is_loopback_request,
    json_default,
    normalize_live_announcement_item,
    normalize_raid_auth_target,
    normalize_raid_state_payload,
    parse_allowlist_ids,
    parse_bool,
    safe_bad_request_detail,
)


class InternalApiContractsPolicyTests(unittest.TestCase):
    def test_callbacks_coalesce_preserves_base_and_overrides(self) -> None:
        base = InternalApiCallbacks(add=lambda _login, _require_link: None)
        merged = InternalApiCallbacks.coalesce(
            base,
            remove_cb=lambda _login: None,
        )

        self.assertIs(base.add, merged.add)
        self.assertIsNone(merged.streamers)
        self.assertIsNotNone(merged.remove)
        self.assertEqual(IdempotencyInFlight("fp", object(), 0.0).fingerprint, "fp")

    def test_policy_host_and_token_helpers(self) -> None:
        self.assertEqual(host_without_port("127.0.0.1:8080"), "127.0.0.1")
        self.assertTrue(is_loopback_host("localhost"))
        self.assertTrue(compare_internal_token("token", "token"))
        self.assertFalse(compare_internal_token("token", "other"))
        self.assertEqual(parse_allowlist_ids("1, 2;3"), {1, 2, 3})
        self.assertEqual(coerce_optional_positive_int("42", key="guild_id"), 42)
        self.assertTrue(
            is_loopback_request(
                request_host="127.0.0.1",
                peer_host="127.0.0.1",
                trusted_proxy_networks=(ipaddress.ip_network("127.0.0.1/32"),),
            )
        )

    def test_policy_normalization_helpers(self) -> None:
        self.assertEqual(normalize_raid_auth_target("discord:123"), "discord:123")
        self.assertEqual(
            normalize_live_announcement_item(
                {
                    "streamer_login": "partner_one",
                    "message_id": 1,
                    "channel_id": 2,
                    "tracking_token": "abc",
                    "referral_url": "https://example.com",
                    "button_label": "Watch now",
                }
            )["streamer_login"],
            "partner_one",
        )
        self.assertEqual(
            normalize_raid_state_payload(
                {
                    "discord_user_id": "123",
                    "twitch_login": "partner_one",
                    "twitch_user_id": "456",
                    "partner_opt_out": False,
                    "token_blacklisted": False,
                    "raid_blacklisted": False,
                    "authorized": True,
                    "blocked": False,
                },
                discord_user_id="123",
                twitch_login="partner_one",
            )["authorized"],
            True,
        )
        self.assertTrue(parse_bool("yes"))
        self.assertEqual(safe_bad_request_detail(ValueError("invalid input")), "invalid input")
        self.assertCountEqual(json_default({1, 2}), [1, 2])
        self.assertEqual(json_default(dt.datetime(2026, 1, 1, 12, 0, 0)), "2026-01-01T12:00:00")
        self.assertEqual(json_default(UUID(int=0)), "00000000-0000-0000-0000-000000000000")


if __name__ == "__main__":
    unittest.main()
