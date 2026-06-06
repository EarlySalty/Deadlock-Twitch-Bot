from __future__ import annotations

import unittest

import discord

from bot.chat.self_explainer import SelfExplainerAnswer
from bot.dashboard.routes_self_explainer import (
    _RateLimiter,
    _build_discord_embed,
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


class DiscordEmbedTests(unittest.TestCase):
    def test_embed_grounded_is_green(self) -> None:
        ans = SelfExplainerAnswer("Antwort", grounded=True, flagged_injection=False)
        embed = _build_discord_embed("Frage?", ans, "1.2.3.4")
        self.assertEqual(embed.color, discord.Color.green())
        names = [f.name for f in embed.fields]
        self.assertIn("Frage", names)
        self.assertIn("Antwort", names)

    def test_embed_injection_is_red_and_flagged(self) -> None:
        ans = SelfExplainerAnswer("x", grounded=True, flagged_injection=True)
        embed = _build_discord_embed("q", ans, "ip")
        self.assertEqual(embed.color, discord.Color.red())
        self.assertTrue(any("Injection" in (f.value or "") for f in embed.fields))


if __name__ == "__main__":
    unittest.main()
