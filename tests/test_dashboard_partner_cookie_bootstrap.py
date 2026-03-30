from __future__ import annotations

import json
import time
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
    headers: dict[str, str] | None = None,
    post_data: dict[str, str] | None = None,
    json_data: dict[str, object] | None = None,
) -> SimpleNamespace:
    request_headers = {
        "Host": "dashboard.example",
        "User-Agent": user_agent,
        **(headers or {}),
    }

    async def _post():
        return post_data or {}

    async def _json():
        if json_data is None:
            raise AssertionError("json() should not be called for this request")
        return json_data

    return SimpleNamespace(
        path=path,
        query=query or {},
        cookies=cookies or {},
        headers=request_headers,
        secure=True,
        remote="203.0.113.10",
        transport=None,
        rel_url=SimpleNamespace(path_qs=path),
        host="dashboard.example",
        post=_post,
        json=_json,
    )


class DashboardPartnerCookieBootstrapTests(unittest.IsolatedAsyncioTestCase):
    def _make_server(self) -> DashboardV2Server:
        server = DashboardV2Server(
            app_token="admin-secret",
            noauth=False,
            partner_token="partner-secret",
            oauth_client_id="client-id",
            oauth_client_secret="client-secret",
            oauth_redirect_uri="https://dashboard.example/twitch/auth/callback",
        )
        server._check_rate_limit = lambda *args, **kwargs: True  # type: ignore[method-assign]
        return server

    def _install_partner_login_state_repo(self, server: DashboardV2Server) -> dict[str, dict[str, object]]:
        repo = server._dashboard_auth_state_repo()
        state_store: dict[str, dict[str, object]] = {}

        def save_partner_login_state(
            *,
            state: str,
            payload: dict[str, object],
            ttl_seconds: float,
            now: float | None = None,
        ) -> None:
            del ttl_seconds
            current = time.time() if now is None else float(now)
            state_store[state] = {
                **payload,
                "created_at": float(payload.get("created_at", current) or current),
                "expires_at": float(payload.get("expires_at", current + 60.0) or (current + 60.0)),
            }

        def consume_partner_login_state(
            state: str,
            *,
            now: float | None = None,
        ) -> dict[str, object] | None:
            current = time.time() if now is None else float(now)
            payload = state_store.pop(state, None)
            if not isinstance(payload, dict):
                return None
            if float(payload.get("expires_at", 0.0) or 0.0) <= current:
                return None
            return dict(payload)

        repo.save_partner_login_state = save_partner_login_state  # type: ignore[method-assign]
        repo.consume_partner_login_state = consume_partner_login_state  # type: ignore[method-assign]
        return state_store

    async def _issue_partner_login_link(
        self,
        server: DashboardV2Server,
        *,
        next_path: str = "/twitch/dashboard-v2",
        headers: dict[str, str] | None = None,
    ) -> dict[str, object]:
        self._install_partner_login_state_repo(server)
        request = _make_request(
            path="/twitch/auth/partner/link",
            query={"next": next_path},
            headers={"X-Admin-Token": "admin-secret", **(headers or {})},
        )
        response = await server.auth_partner_link(request)
        self.assertEqual(response.status, 200)
        self.assertEqual(response.headers.get("Cache-Control"), "no-store, max-age=0")
        return json.loads(response.text)

    async def _bootstrap_partner_cookie(
        self,
        server: DashboardV2Server,
        *,
        next_path: str = "/twitch/dashboard-v2",
        user_agent: str = "CodexTest/1.0",
    ) -> str:
        issued_link = await self._issue_partner_login_link(server, next_path=next_path)
        token = str(issued_link["login_token"])
        self.assertTrue(token)

        login_request = _make_request(
            path="/twitch/auth/partner/login",
            user_agent=user_agent,
            post_data={"token": token},
        )
        with self.assertRaises(web.HTTPSeeOther) as ctx:
            await server.auth_partner_login(login_request)

        response = ctx.exception
        session_cookie = response.cookies.get(server._partner_access_cookie_name())
        self.assertIsNotNone(session_cookie)
        self.assertTrue(bool(session_cookie.value))
        return session_cookie.value

    async def test_partner_login_link_returns_signed_login_url(self) -> None:
        server = self._make_server()

        response_payload = await self._issue_partner_login_link(
            server,
            next_path="/twitch/dashboard-v2?streamer=midcore_live",
        )

        self.assertEqual(response_payload["login_path"], "/twitch/auth/partner/login")
        self.assertEqual(response_payload["login_method"], "POST")
        self.assertTrue(str(response_payload["login_token"]))
        self.assertEqual(
            response_payload["next_path"],
            "/twitch/dashboard-v2?streamer=midcore_live",
        )
        self.assertGreater(int(response_payload["expires_in"]), 0)

    async def test_partner_login_sets_cookie_and_redirects_without_query_bootstrap(self) -> None:
        server = self._make_server()
        issued_link = await self._issue_partner_login_link(
            server,
            next_path="/twitch/dashboard-v2?streamer=midcore_live",
        )
        token = str(issued_link["login_token"])

        login_request = _make_request(
            path="/twitch/auth/partner/login",
            post_data={"token": token},
        )

        with self.assertRaises(web.HTTPSeeOther) as ctx:
            await server.auth_partner_login(login_request)

        response = ctx.exception
        self.assertEqual(response.location, "/twitch/dashboard-v2?streamer=midcore_live")
        self.assertEqual(response.headers.get("Cache-Control"), "no-store, max-age=0")
        cookie = response.cookies.get(server._partner_access_cookie_name())
        self.assertIsNotNone(cookie)
        self.assertTrue(bool(cookie.value))
        self.assertTrue(cookie["httponly"])

    async def test_partner_login_token_is_one_time(self) -> None:
        server = self._make_server()
        issued_link = await self._issue_partner_login_link(server)
        token = str(issued_link["login_token"])

        with self.assertRaises(web.HTTPSeeOther):
            await server.auth_partner_login(
                _make_request(
                    path="/twitch/auth/partner/login",
                    post_data={"token": token},
                )
            )

        replay_response = await server.auth_partner_login(
            _make_request(
                path="/twitch/auth/partner/login",
                post_data={"token": token},
            )
        )
        self.assertEqual(replay_response.status, 401)
        self.assertIn("ungültig oder abgelaufen", replay_response.text)

    async def test_partner_login_token_rejects_tampering(self) -> None:
        server = self._make_server()
        issued_link = await self._issue_partner_login_link(server)
        token = str(issued_link["login_token"])
        tampered_token = f"{token[:-1]}A" if token else "invalid"

        response = await server.auth_partner_login(
            _make_request(
                path="/twitch/auth/partner/login",
                post_data={"token": tampered_token},
            )
        )
        self.assertEqual(response.status, 401)
        self.assertIn("ungültig oder abgelaufen", response.text)

    async def test_partner_login_requires_admin_or_local_issuer_credentials(self) -> None:
        server = self._make_server()
        self._install_partner_login_state_repo(server)

        request = _make_request(
            path="/twitch/auth/partner/link",
            query={"next": "/twitch/dashboard-v2"},
            headers={"X-Partner-Token": "partner-secret"},
        )

        with self.assertRaises(web.HTTPUnauthorized):
            await server.auth_partner_link(request)

    async def test_partner_login_fails_closed_when_state_persistence_fails(self) -> None:
        server = self._make_server()
        repo = server._dashboard_auth_state_repo()

        def _save_partner_login_state(**kwargs) -> None:
            del kwargs
            raise RuntimeError("db-down")

        repo.save_partner_login_state = _save_partner_login_state  # type: ignore[method-assign]
        request = _make_request(
            path="/twitch/auth/partner/link",
            query={"next": "/twitch/dashboard-v2"},
            headers={"X-Admin-Token": "admin-secret"},
        )

        response = await server.auth_partner_link(request)
        self.assertEqual(response.status, 503)

    async def test_partner_login_fails_closed_when_persisted_consume_errors(self) -> None:
        server = self._make_server()
        self._install_partner_login_state_repo(server)
        issued_link = await self._issue_partner_login_link(server)
        token = str(issued_link["login_token"])

        repo = server._dashboard_auth_state_repo()
        original_consume = repo.consume_partner_login_state

        def _consume_partner_login_state_fail(*args, **kwargs):
            raise RuntimeError("db-read-failed")

        repo.consume_partner_login_state = _consume_partner_login_state_fail  # type: ignore[method-assign]
        failed_response = await server.auth_partner_login(
            _make_request(
                path="/twitch/auth/partner/login",
                post_data={"token": token},
            )
        )
        self.assertEqual(failed_response.status, 401)

        repo.consume_partner_login_state = original_consume  # type: ignore[method-assign]
        with self.assertRaises(web.HTTPSeeOther):
            await server.auth_partner_login(
                _make_request(
                    path="/twitch/auth/partner/login",
                    post_data={"token": token},
                )
            )

    async def test_partner_login_consumes_persisted_state_without_in_memory_cache(self) -> None:
        server = self._make_server()
        self._install_partner_login_state_repo(server)
        issued_link = await self._issue_partner_login_link(server)
        token = str(issued_link["login_token"])
        server._dashboard_auth_state_cache("_partner_login_states").clear()

        with self.assertRaises(web.HTTPSeeOther):
            await server.auth_partner_login(
                _make_request(
                    path="/twitch/auth/partner/login",
                    post_data={"token": token},
                )
            )

    async def test_partner_login_endpoints_are_rate_limited(self) -> None:
        server = self._make_server()
        self._install_partner_login_state_repo(server)
        server._check_rate_limit = lambda *args, **kwargs: False  # type: ignore[method-assign]

        issue_response = await server.auth_partner_link(
            _make_request(
                path="/twitch/auth/partner/link",
                query={"next": "/twitch/dashboard-v2"},
                headers={"X-Admin-Token": "admin-secret"},
            )
        )
        self.assertEqual(issue_response.status, 429)

        login_response = await server.auth_partner_login(
            _make_request(
                path="/twitch/auth/partner/login",
                post_data={"token": "unused"},
            )
        )
        self.assertEqual(login_response.status, 429)

    async def test_partner_link_requires_same_origin_for_admin_session_issuance(self) -> None:
        server = self._make_server()
        self._install_partner_login_state_repo(server)
        server._get_auth_level = lambda _request: "admin"  # type: ignore[method-assign]
        server._is_discord_admin_request = lambda _request: False  # type: ignore[method-assign]

        with self.assertRaises(web.HTTPForbidden):
            await server.auth_partner_link(
                _make_request(
                    path="/twitch/auth/partner/link",
                    query={"next": "/twitch/dashboard-v2"},
                )
            )

    async def test_partner_link_accepts_same_origin_for_admin_session_issuance(self) -> None:
        server = self._make_server()
        self._install_partner_login_state_repo(server)
        server._get_auth_level = lambda _request: "admin"  # type: ignore[method-assign]
        server._is_discord_admin_request = lambda _request: False  # type: ignore[method-assign]

        response = await server.auth_partner_link(
            _make_request(
                path="/twitch/auth/partner/link",
                query={"next": "/twitch/dashboard-v2"},
                headers={"Origin": "https://dashboard.example"},
            )
        )
        self.assertEqual(response.status, 200)

    async def test_partner_cookie_session_is_bound_to_request_fingerprint(self) -> None:
        server = self._make_server()
        session_cookie = await self._bootstrap_partner_cookie(server)

        valid_request = _make_request(cookies={server._partner_access_cookie_name(): session_cookie})
        self.assertEqual(AnalyticsV2Mixin._get_auth_level(server, valid_request), "partner")

        hijacked_request = _make_request(
            cookies={server._partner_access_cookie_name(): session_cookie},
            user_agent="DifferentAgent/9.9",
        )
        self.assertEqual(AnalyticsV2Mixin._get_auth_level(server, hijacked_request), "none")

    async def test_partner_cookie_survives_client_host_change_when_browser_fingerprint_is_stable(
        self,
    ) -> None:
        server = self._make_server()
        session_cookie = await self._bootstrap_partner_cookie(server)

        mobile_request = _make_request(cookies={server._partner_access_cookie_name(): session_cookie})
        mobile_request.remote = "198.51.100.24"
        self.assertEqual(AnalyticsV2Mixin._get_auth_level(server, mobile_request), "partner")

    async def test_dashboard_page_ignores_legacy_partner_query_bootstrap(self) -> None:
        server = self._make_server()
        request = _make_request(
            path="/twitch/dashboard",
            query={"partner_token": "partner-secret", "streamer": "midcore_live"},
        )

        with self.assertRaises(web.HTTPFound) as ctx:
            await server._serve_dashboard(request)

        response = ctx.exception
        self.assertNotIn("partner_token=", response.location)
        self.assertIn("/twitch/auth/login", response.location)

    async def test_logout_clears_partner_cookie_session(self) -> None:
        server = self._make_server()
        session_cookie = await self._bootstrap_partner_cookie(server)

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
