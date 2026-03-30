import unittest

from aiohttp import web
from aiohttp.test_utils import TestClient, TestServer
from unittest.mock import patch

from bot.dashboard.admin.legal_mixin import _DashboardLegalMixin
from bot.dashboard.server_v2 import build_v2_app


class _DummyLegalServer(_DashboardLegalMixin):
    pass


class DashboardLegalAccessTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.server_impl = _DummyLegalServer()
        self.app = web.Application()
        self.app.add_routes(
            [
                web.get("/robots.txt", self.server_impl.robots_txt),
                web.get("/twitch/impressum", self.server_impl.abbo_impressum),
                web.get("/twitch/datenschutz", self.server_impl.abbo_datenschutz),
            ]
        )

    async def test_robots_txt_disallows_legal_pages(self) -> None:
        async with TestServer(self.app) as server:
            async with TestClient(server) as client:
                response = await client.get("/robots.txt")
                body = await response.text()

        self.assertEqual(response.status, 200)
        self.assertIn("Disallow: /twitch/impressum", body)
        self.assertIn("Disallow: /twitch/datenschutz", body)

    async def test_build_v2_app_registers_robots_txt_route(self) -> None:
        with (
            patch("bot.dashboard.server_v2.storage_pg.prepare_runtime_storage"),
            patch("bot.dashboard.affiliate.affiliate_mixin._DashboardAffiliateMixin._affiliate_register_routes"),
            patch("bot.dashboard.routes_mixin._DashboardRoutesMixin._register_social_media_routes"),
        ):
            app = build_v2_app(noauth=True, token="secret")
            async with TestServer(app) as server:
                async with TestClient(server) as client:
                    response = await client.get("/robots.txt")
                    body = await response.text()

        self.assertEqual(response.status, 200)
        self.assertIn("Disallow: /twitch/impressum", body)

    async def test_human_browser_can_open_impressum(self) -> None:
        async with TestServer(self.app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    "/twitch/impressum",
                    headers={
                        "User-Agent": (
                            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
                            "AppleWebKit/537.36 (KHTML, like Gecko) "
                            "Chrome/134.0.0.0 Safari/537.36"
                        )
                    },
                )
                body = await response.text()

        self.assertEqual(response.status, 200)
        self.assertIn("Impressum", body)
        self.assertEqual(
            response.headers.get("X-Robots-Tag"),
            "noindex, nofollow, noarchive, nosnippet, noimageindex",
        )

    async def test_ai_bot_is_blocked_from_datenschutz(self) -> None:
        async with TestServer(self.app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    "/twitch/datenschutz",
                    headers={"User-Agent": "GPTBot/1.0"},
                )
                body = await response.text()

        self.assertEqual(response.status, 403)
        self.assertEqual(body, "Forbidden")
        self.assertEqual(
            response.headers.get("X-Robots-Tag"),
            "noindex, nofollow, noarchive, nosnippet, noimageindex",
        )
