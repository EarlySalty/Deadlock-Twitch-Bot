from __future__ import annotations

import unittest

from bot.dashboard import routes_entry
from bot.dashboard.server_v2 import DashboardV2Server


class _DummyServer:
    async def public_home(self, *_args, **_kwargs):
        return None

    async def legacy_dashboard_redirect(self, *_args, **_kwargs):
        return None

    async def index(self, *_args, **_kwargs):
        return None

    async def admin(self, *_args, **_kwargs):
        return None

    async def legacy_admin(self, *_args, **_kwargs):
        return None

    async def admin_announcements_page(self, *_args, **_kwargs):
        return None

    async def admin_announcements_save(self, *_args, **_kwargs):
        return None

    async def admin_roadmap_page(self, *_args, **_kwargs):
        return None

    async def live_announcement_page(self, *_args, **_kwargs):
        return None

    async def add_any(self, *_args, **_kwargs):
        return None

    async def add_url(self, *_args, **_kwargs):
        return None

    async def add_login(self, *_args, **_kwargs):
        return None

    async def add_streamer(self, *_args, **_kwargs):
        return None

    async def admin_partner_chat_action(self, *_args, **_kwargs):
        return None

    async def admin_manual_plan_save(self, *_args, **_kwargs):
        return None

    async def admin_manual_plan_clear(self, *_args, **_kwargs):
        return None

    async def remove(self, *_args, **_kwargs):
        return None

    async def verify(self, *_args, **_kwargs):
        return None

    async def archive(self, *_args, **_kwargs):
        return None

    async def discord_flag(self, *_args, **_kwargs):
        return None

    async def partner_stats(self, *_args, **_kwargs):
        return None

    async def auth_logout(self, *_args, **_kwargs):
        return None

    async def discord_link(self, *_args, **_kwargs):
        return None

    async def reload_cog(self, *_args, **_kwargs):
        return None


class DashboardPublicRouteRegressionTests(unittest.TestCase):
    def test_entry_routes_do_not_claim_dashboard_v2_public_path(self) -> None:
        server = _DummyServer()
        routes = routes_entry.build_route_defs(server)
        claimed_paths = {route.path for route in routes}
        route_map = {route.path: route for route in routes}

        self.assertNotIn("/twitch/dashboard-v2", claimed_paths)
        self.assertIn("/twitch/stats", claimed_paths)
        self.assertEqual(
            route_map["/twitch/stats"].handler.__func__,
            server.legacy_dashboard_redirect.__func__,
        )

    def test_legacy_stats_url_stays_on_public_dashboard_surface(self) -> None:
        server = DashboardV2Server(
            app_token=None,
            noauth=False,
            partner_token=None,
            oauth_client_id=None,
            oauth_client_secret=None,
            oauth_redirect_uri="https://deutsche-deadlock-community.de/twitch/auth/callback",
        )

        self.assertEqual(server._resolve_legacy_stats_url(), "/twitch/dashboard")

    def test_public_dashboard_login_url_does_not_fall_back_to_admin_discord_oauth(self) -> None:
        server = DashboardV2Server(
            app_token=None,
            noauth=False,
            partner_token=None,
            oauth_client_id=None,
            oauth_client_secret=None,
            oauth_redirect_uri="https://deutsche-deadlock-community.de/twitch/auth/callback",
        )
        server._discord_admin_required = True
        request = type(
            "Req",
            (),
            {
                "path": "/twitch/dashboard",
                "rel_url": type("Rel", (), {"path_qs": "/twitch/dashboard"})(),
            },
        )()

        self.assertEqual(
            server._build_dashboard_login_url(request),
            "/twitch/auth/login?next=%2Ftwitch%2Fdashboard",
        )

    def test_public_host_discord_admin_login_is_not_exposed(self) -> None:
        import asyncio

        server = DashboardV2Server(
            app_token=None,
            noauth=False,
            partner_token=None,
            oauth_client_id=None,
            oauth_client_secret=None,
            oauth_redirect_uri="https://deutsche-deadlock-community.de/twitch/auth/callback",
        )
        server._discord_admin_required = True
        request = type(
            "Req",
            (),
            {
                "headers": {"Host": "deutsche-deadlock-community.de"},
                "host": "deutsche-deadlock-community.de",
                "path": "/twitch/auth/discord/login",
                "query": {"next": "/twitch/dashboard"},
                "cookies": {},
                "remote": "203.0.113.10",
                "transport": None,
            },
        )()

        response = asyncio.run(server.discord_auth_login(request))

        self.assertEqual(response.status, 404)
