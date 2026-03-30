from __future__ import annotations

import unittest
from types import SimpleNamespace

from aiohttp import web

from bot.analytics.api_v2 import AnalyticsV2Mixin
from bot.dashboard.server_v2 import DashboardV2Server


def _make_request(
    *,
    path: str = "/twitch/dashboard-v2",
    query: dict[str, str] | None = None,
    cookies: dict[str, str] | None = None,
    user_agent: str = "CodexTest/1.0",
) -> SimpleNamespace:
    return SimpleNamespace(
        path=path,
        query=query or {},
        cookies=cookies or {},
        headers={"Host": "dashboard.example", "User-Agent": user_agent},
        secure=True,
        remote="203.0.113.10",
        transport=None,
        rel_url=SimpleNamespace(path_qs=path),
        host="dashboard.example",
    )


class DashboardPartnerCookieBootstrapTests(unittest.IsolatedAsyncioTestCase):
    def _make_server(self) -> DashboardV2Server:
        return DashboardV2Server(
            app_token="admin-secret",
            noauth=False,
            partner_token="partner-secret",
            oauth_client_id="client-id",
            oauth_client_secret="client-secret",
            oauth_redirect_uri="https://dashboard.example/twitch/auth/callback",
        )

    def test_partner_query_bootstrap_sets_cookie_and_scrubs_url(self) -> None:
        server = self._make_server()
        request = _make_request(
            query={"partner_token": "partner-secret", "streamer": "midcore_live"},
        )

        response = server._consume_partner_token_bootstrap(request)

        self.assertIsInstance(response, web.HTTPFound)
        assert isinstance(response, web.HTTPFound)
        self.assertEqual(response.location, "/twitch/dashboard-v2?streamer=midcore_live")
        cookie = response.cookies.get(server._partner_access_cookie_name())
        self.assertIsNotNone(cookie)
        self.assertTrue(bool(cookie.value))
        self.assertTrue(cookie["httponly"])

    def test_partner_cookie_session_is_bound_to_request_fingerprint(self) -> None:
        server = self._make_server()
        bootstrap_request = _make_request(query={"partner_token": "partner-secret"})
        response = server._consume_partner_token_bootstrap(bootstrap_request)

        self.assertIsInstance(response, web.HTTPFound)
        assert isinstance(response, web.HTTPFound)
        session_cookie = response.cookies[server._partner_access_cookie_name()].value

        valid_request = _make_request(cookies={server._partner_access_cookie_name(): session_cookie})
        self.assertEqual(AnalyticsV2Mixin._get_auth_level(server, valid_request), "partner")

        hijacked_request = _make_request(
            cookies={server._partner_access_cookie_name(): session_cookie},
            user_agent="DifferentAgent/9.9",
        )
        self.assertEqual(AnalyticsV2Mixin._get_auth_level(server, hijacked_request), "none")

    def test_partner_cookie_survives_client_host_change_when_browser_fingerprint_is_stable(self) -> None:
        server = self._make_server()
        bootstrap_request = _make_request(query={"partner_token": "partner-secret"})
        response = server._consume_partner_token_bootstrap(bootstrap_request)

        self.assertIsInstance(response, web.HTTPFound)
        assert isinstance(response, web.HTTPFound)
        session_cookie = response.cookies[server._partner_access_cookie_name()].value

        mobile_request = _make_request(cookies={server._partner_access_cookie_name(): session_cookie})
        mobile_request.remote = "198.51.100.24"
        self.assertEqual(AnalyticsV2Mixin._get_auth_level(server, mobile_request), "partner")

    async def test_logout_clears_partner_cookie_session(self) -> None:
        server = self._make_server()
        bootstrap_request = _make_request(query={"partner_token": "partner-secret"})
        response = server._consume_partner_token_bootstrap(bootstrap_request)

        self.assertIsInstance(response, web.HTTPFound)
        assert isinstance(response, web.HTTPFound)
        session_cookie = response.cookies[server._partner_access_cookie_name()].value

        logout_request = _make_request(
            path="/twitch/auth/logout",
            cookies={server._partner_access_cookie_name(): session_cookie},
        )
        logout_request.rel_url = SimpleNamespace(path_qs="/twitch/auth/logout")

        with self.assertRaises(web.HTTPFound) as ctx:
            await server.auth_logout(logout_request)

        cleared_cookie = ctx.exception.cookies.get(server._partner_access_cookie_name())
        self.assertIsNotNone(cleared_cookie)
        self.assertEqual(cleared_cookie.value, "")


if __name__ == "__main__":
    unittest.main()
