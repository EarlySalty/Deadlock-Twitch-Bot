from __future__ import annotations

import unittest

from bot.chat.self_explainer import SelfExplainerAnswer
from bot.dashboard.routes_self_explainer import (
    _RateLimiter,
    _build_discord_payload,
    build_route_defs,
)


class RateLimiterTests(unittest.TestCase):
    def test_allows_up_to_max_then_blocks(self) -> None:
        rl = _RateLimiter(window_sec=60.0, max_hits=3)
        self.assertTrue(rl.allow("ip", 0.0))
        self.assertTrue(rl.allow("ip", 1.0))
        self.assertTrue(rl.allow("ip", 2.0))
        self.assertFalse(rl.allow("ip", 3.0))

    def test_window_expiry_resets(self) -> None:
        rl = _RateLimiter(window_sec=60.0, max_hits=1)
        self.assertTrue(rl.allow("ip", 0.0))
        self.assertFalse(rl.allow("ip", 30.0))
        self.assertTrue(rl.allow("ip", 61.0))

    def test_per_peer_isolation(self) -> None:
        rl = _RateLimiter(window_sec=60.0, max_hits=1)
        self.assertTrue(rl.allow("a", 0.0))
        self.assertTrue(rl.allow("b", 0.0))
        self.assertFalse(rl.allow("a", 1.0))


class RouteDefTests(unittest.TestCase):
    def test_build_route_defs(self) -> None:
        defs = build_route_defs(object())
        self.assertEqual(len(defs), 1)
        route = defs[0]
        self.assertEqual(route.method, "POST")
        self.assertIn("self-explainer/ask", route.path)


class DiscordPayloadTests(unittest.TestCase):
    def test_payload_grounded_is_green(self) -> None:
        ans = SelfExplainerAnswer("Antwort", grounded=True, flagged_injection=False)
        payload = _build_discord_payload("Frage?", ans, "1.2.3.4")
        embed = payload["embeds"][0]
        self.assertEqual(embed["color"], 0x57F287)
        names = [f["name"] for f in embed["fields"]]
        self.assertIn("Frage", names)
        self.assertIn("Antwort", names)

    def test_payload_injection_is_red_and_flagged(self) -> None:
        ans = SelfExplainerAnswer("x", grounded=True, flagged_injection=True)
        embed = _build_discord_payload("q", ans, "ip")["embeds"][0]
        self.assertEqual(embed["color"], 0xED4245)
        self.assertTrue(any("Injection" in f["value"] for f in embed["fields"]))


if __name__ == "__main__":
    unittest.main()
