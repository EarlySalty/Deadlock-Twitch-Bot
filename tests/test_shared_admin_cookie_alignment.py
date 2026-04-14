from __future__ import annotations

import hashlib
import unittest
from types import SimpleNamespace

from aiohttp import web
from aiohttp.test_utils import make_mocked_request

from bot.dashboard import routes_entry
from bot.dashboard.server_v2 import DashboardV2Server


def _make_request(
    *,
    path: str = "/twitch/auth/validate",
    cookies: dict[str, str] | None = None,
    headers: dict[str, str] | None = None,
    host: str = "admin.deutsche-deadlock-community.de",
    peer_host: str = "127.0.0.1",
    client_host: str | None = None,
    post_data: dict[str, str] | None = None,
) -> SimpleNamespace:
    async def _post() -> dict[str, str]:
        return post_data or {}

    request_headers = {
        "User-Agent": "CodexTest/1.0",
        "Accept-Language": "de-DE",
        "Sec-CH-UA-Platform": '"Linux"',
    }
    if headers:
        request_headers.update(headers)

    return SimpleNamespace(
        path=path,
        query={},
        rel_url=SimpleNamespace(path_qs=path),
        cookies=cookies or {},
        headers=request_headers,
        host=host,
        remote=peer_host,
        transport=None,
        secure=True,
        _fake_client_ip=client_host,
        post=_post,
    )


def _passive_fp_hash(ua: str, lang: str = "", platform: str = "") -> str:
    raw = f"{ua}|{lang}|{platform}"
    return hashlib.sha256(raw.encode()).hexdigest()[:32]


def _build_server() -> DashboardV2Server:
    server = DashboardV2Server(
        app_token=None,
        noauth=False,
        partner_token=None,
        oauth_client_id=None,
        oauth_client_secret=None,
        oauth_redirect_uri="https://dashboard.example/twitch/auth/callback",
    )
    server._discord_admin_cookie_name = "twitch_dash_session"
    server._session_cookie_name = "twitch_dash_session"
    server._discord_admin_session_ttl = 24 * 3600
    return server


class SharedAdminCookieAlignmentTests(unittest.IsolatedAsyncioTestCase):
    def _skip_until_validate_exists(self) -> None:
        if not hasattr(DashboardV2Server, "validate_admin_session"):
            self.skipTest("validate_admin_session is not implemented yet")

    def _skip_until_fingerprint_exists(self) -> None:
        if not hasattr(DashboardV2Server, "fingerprint_page") or not hasattr(
            DashboardV2Server, "fingerprint_submit"
        ):
            self.skipTest("fingerprint flow handlers are not implemented yet")

    async def test_validate_returns_401_for_unauthenticated(self) -> None:
        self._skip_until_validate_exists()
        server = _build_server()
        request = _make_request()

        server._get_discord_admin_session = lambda req: None  # type: ignore[method-assign]
        server._get_dashboard_auth_session = lambda req: {}  # type: ignore[method-assign]
        response = await DashboardV2Server.validate_admin_session(server, request)

        self.assertEqual(response.status, 401)

    async def test_validate_allows_localhost_without_cookie_refresh(self) -> None:
        self._skip_until_validate_exists()
        server = _build_server()
        request = _make_request(host="127.0.0.1", peer_host="127.0.0.1")

        server._get_discord_admin_session = lambda req: None  # type: ignore[method-assign]
        server._get_dashboard_auth_session = lambda req: {}  # type: ignore[method-assign]
        response = await DashboardV2Server.validate_admin_session(server, request)

        self.assertEqual(response.status, 200)
        self.assertIsNone(response.cookies.get("twitch_dash_session"))

    async def test_validate_refreshes_cookie_for_admin_session(self) -> None:
        self._skip_until_validate_exists()
        server = _build_server()
        request = _make_request(
            cookies={"twitch_dash_session": "abc123"},
            headers={"X-Forwarded-For": "10.0.0.1"},
        )
        session = {
            "username": "Admin User",
            "client_ip": "10.0.0.1",
            "passive_fp": _passive_fp_hash("CodexTest/1.0", "de-DE", "Linux"),
            "fp_pending": False,
            "js_fp": "c0ffee",
        }

        server._get_discord_admin_session = lambda req: session  # type: ignore[method-assign]
        server._get_dashboard_auth_session = lambda req: {}  # type: ignore[method-assign]
        response = await DashboardV2Server.validate_admin_session(server, request)

        self.assertEqual(response.status, 200)
        cookie = response.cookies.get("twitch_dash_session")
        self.assertIsNotNone(cookie)
        self.assertEqual(cookie["max-age"], str(24 * 3600))
        self.assertTrue(cookie["httponly"])
        self.assertEqual(str(cookie["samesite"]).lower(), "lax")

    async def test_validate_rejects_ip_mismatch_when_session_is_bound(self) -> None:
        self._skip_until_validate_exists()
        server = _build_server()
        request = _make_request(
            cookies={"twitch_dash_session": "abc123"},
            headers={"X-Forwarded-For": "5.6.7.8"},
        )
        server._get_discord_admin_session = lambda req: {
            "client_ip": "10.0.0.1",
            "passive_fp": _passive_fp_hash("CodexTest/1.0", "de-DE", "Linux"),
            "fp_pending": False,
            "js_fp": "c0ffee",
        }
        server._get_dashboard_auth_session = lambda req: {}  # type: ignore[method-assign]

        response = await DashboardV2Server.validate_admin_session(server, request)

        self.assertEqual(response.status, 401)

    async def test_validate_rejects_passive_fingerprint_mismatch(self) -> None:
        self._skip_until_validate_exists()
        server = _build_server()
        request = _make_request(
            cookies={"twitch_dash_session": "abc123"},
            headers={
                "User-Agent": "Mozilla/5.0 (Macintosh)",
                "Accept-Language": "de-DE",
                "Sec-CH-UA-Platform": '"macOS"',
            },
            client_host="10.0.0.1",
        )
        server._get_discord_admin_session = lambda req: {
            "client_ip": "10.0.0.1",
            "passive_fp": _passive_fp_hash("Mozilla/5.0 (Linux)", "de-DE", "Linux"),
            "fp_pending": False,
            "js_fp": "c0ffee",
        }
        server._get_dashboard_auth_session = lambda req: {}  # type: ignore[method-assign]

        response = await DashboardV2Server.validate_admin_session(server, request)

        self.assertEqual(response.status, 401)

    async def test_validate_rejects_fp_pending_sessions(self) -> None:
        self._skip_until_validate_exists()
        server = _build_server()
        request = _make_request(
            cookies={"twitch_dash_session": "abc123"},
            client_host="10.0.0.1",
        )
        server._get_discord_admin_session = lambda req: {
            "client_ip": "10.0.0.1",
            "passive_fp": _passive_fp_hash("CodexTest/1.0", "de-DE", "Linux"),
            "fp_pending": True,
            "js_fp": "c0ffee",
        }  # type: ignore[method-assign]
        server._get_dashboard_auth_session = lambda req: {}  # type: ignore[method-assign]

        response = await DashboardV2Server.validate_admin_session(server, request)

        self.assertEqual(response.status, 401)

    async def test_fingerprint_page_renders_html(self) -> None:
        self._skip_until_fingerprint_exists()
        server = _build_server()
        server._dashboard_auth_state_repo = lambda: SimpleNamespace()  # type: ignore[method-assign]
        server._get_discord_admin_session = lambda req: {"fp_pending": True}  # type: ignore[method-assign]
        request = make_mocked_request(
            "GET",
            "/twitch/auth/fingerprint",
            headers={"Cookie": "twitch_dash_session=abc"},
        )

        response = await DashboardV2Server.fingerprint_page(server, request)

        self.assertEqual(response.status, 200)
        body = response.text if hasattr(response, "text") else ""
        self.assertIn("fingerprint", body.lower())

    async def test_fingerprint_submit_stores_fp_and_clears_pending(self) -> None:
        self._skip_until_fingerprint_exists()
        server = _build_server()
        session_store = {"fp_pending": True}
        persisted: list[tuple[str, dict[str, object]]] = []

        class _Repo:
            def save_discord_admin_session(self, *, session_id, payload, created_at, expires_at):
                persisted.append(
                    (
                        str(session_id),
                        {
                            "payload": dict(payload),
                            "created_at": created_at,
                            "expires_at": expires_at,
                        },
                    )
                )

        server._dashboard_auth_state_repo = lambda: _Repo()  # type: ignore[method-assign]
        server._get_discord_admin_session = lambda req: session_store  # type: ignore[method-assign]
        request = _make_request(
            path="/twitch/auth/fingerprint",
            cookies={"twitch_dash_session": "abc"},
            post_data={"fp": "0123456789abcdef0123456789abcdef"},
        )

        with self.assertRaises(web.HTTPSeeOther) as ctx:
            await DashboardV2Server.fingerprint_submit(server, request)

        response = ctx.exception
        self.assertEqual(response.location, "/twitch/admin")
        self.assertEqual(session_store.get("js_fp"), "0123456789abcdef0123456789abcdef")
        self.assertFalse(session_store.get("fp_pending"))
        self.assertTrue(persisted)

    def test_route_defs_will_include_shared_admin_auth_routes(self) -> None:
        server = _build_server()

        route_paths = {route.path for route in routes_entry.build_route_defs(server)}

        self.assertIn("/twitch/auth/validate", route_paths)
        self.assertIn("/twitch/auth/fingerprint", route_paths)


if __name__ == "__main__":
    unittest.main()
