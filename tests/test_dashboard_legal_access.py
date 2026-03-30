import unittest

from aiohttp import web
from aiohttp.test_utils import TestClient, TestServer
from unittest.mock import patch

from bot.dashboard.admin.legal_mixin import (
    LEGAL_GATE_COOKIE_NAME,
    _DashboardLegalMixin,
)
from bot.dashboard.server_v2 import build_v2_app


class _FakeTurnstileResponse:
    def __init__(self, payload: dict[str, object]) -> None:
        self.payload = payload

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc, tb) -> None:
        del exc_type, exc, tb

    async def json(self) -> dict[str, object]:
        return dict(self.payload)


class _FakeTurnstileSession:
    def __init__(self, payload: dict[str, object]) -> None:
        self.payload = payload
        self.calls: list[dict[str, object]] = []

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc, tb) -> None:
        del exc_type, exc, tb

    def post(self, url: str, data: dict[str, object]):
        self.calls.append({"url": url, "data": dict(data)})
        return _FakeTurnstileResponse(self.payload)


class _DummyLegalServer(_DashboardLegalMixin):
    def __init__(
        self,
        *,
        turnstile_site_key: str = "",
        turnstile_secret_key: str = "",
        cookie_secret: str = "",
        verify_ok: bool = True,
    ) -> None:
        self._secrets = {
            "TWITCH_LEGAL_TURNSTILE_SITE_KEY": turnstile_site_key,
            "TWITCH_LEGAL_TURNSTILE_SECRET_KEY": turnstile_secret_key,
            "TWITCH_LEGAL_GATE_COOKIE_SECRET": cookie_secret,
        }
        self.verify_ok = verify_ok
        self.last_verified_token = None

    def _load_secret_value(self, *keys: str) -> str:
        for key in keys:
            value = str(self._secrets.get(key) or "").strip()
            if value:
                return value
        return ""

    @staticmethod
    def _is_secure_request(_request: web.Request) -> bool:
        return False

    async def _verify_legal_turnstile_token(
        self,
        request: web.Request,
        token: str,
    ) -> bool:
        del request
        self.last_verified_token = token
        return self.verify_ok


class DashboardLegalAccessTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.server_impl = _DummyLegalServer()
        self.app = web.Application()
        self.app.add_routes(
            [
                web.get("/robots.txt", self.server_impl.robots_txt),
                web.get("/twitch/legal/access", self.server_impl.legal_access_page),
                web.post("/twitch/legal/verify", self.server_impl.legal_verify),
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

    async def test_build_v2_app_registers_legal_gate_routes(self) -> None:
        with (
            patch("bot.dashboard.server_v2.storage_pg.prepare_runtime_storage"),
            patch("bot.dashboard.affiliate.affiliate_mixin._DashboardAffiliateMixin._affiliate_register_routes"),
            patch("bot.dashboard.routes_mixin._DashboardRoutesMixin._register_social_media_routes"),
        ):
            app = build_v2_app(noauth=True, token="secret")
            async with TestServer(app) as server:
                async with TestClient(server) as client:
                    access_response = await client.get("/twitch/legal/access", allow_redirects=False)
                    verify_response = await client.post("/twitch/legal/verify", allow_redirects=False)

        self.assertNotEqual(access_response.status, 404)
        self.assertNotEqual(verify_response.status, 404)

    async def test_missing_gate_configuration_fails_closed_for_humans(self) -> None:
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

        self.assertEqual(response.status, 503)
        self.assertEqual(body, "Legal access gate is not configured.")
        self.assertEqual(
            response.headers.get("X-Robots-Tag"),
            "noindex, nofollow, noarchive, nosnippet, noimageindex",
        )

    async def test_gate_redirects_human_without_cookie_when_configured(self) -> None:
        server_impl = _DummyLegalServer(
            turnstile_site_key="site-key",
            turnstile_secret_key="secret-key",
            cookie_secret="cookie-secret",
        )
        app = web.Application()
        app.add_routes(
            [
                web.get("/twitch/legal/access", server_impl.legal_access_page),
                web.post("/twitch/legal/verify", server_impl.legal_verify),
                web.get("/twitch/impressum", server_impl.abbo_impressum),
            ]
        )

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    "/twitch/impressum",
                    allow_redirects=False,
                    headers={"User-Agent": "Mozilla/5.0"},
                )

        self.assertEqual(response.status, 302)
        self.assertEqual(
            response.headers.get("Location"),
            "/twitch/legal/access?next=/twitch/impressum",
        )

    async def test_gate_page_renders_turnstile_widget(self) -> None:
        server_impl = _DummyLegalServer(
            turnstile_site_key="site-key",
            turnstile_secret_key="secret-key",
            cookie_secret="cookie-secret",
        )
        app = web.Application()
        app.add_routes([web.get("/twitch/legal/access", server_impl.legal_access_page)])

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get("/twitch/legal/access?next=/twitch/datenschutz")
                body = await response.text()

        self.assertEqual(response.status, 200)
        self.assertIn("site-key", body)
        self.assertIn("cf-turnstile", body)
        self.assertIn("/twitch/datenschutz", body)

    async def test_verify_sets_cookie_and_redirects_to_legal_page(self) -> None:
        server_impl = _DummyLegalServer(
            turnstile_site_key="site-key",
            turnstile_secret_key="secret-key",
            cookie_secret="cookie-secret",
            verify_ok=True,
        )
        app = web.Application()
        app.add_routes(
            [
                web.post("/twitch/legal/verify", server_impl.legal_verify),
                web.get("/twitch/datenschutz", server_impl.abbo_datenschutz),
            ]
        )

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.post(
                    "/twitch/legal/verify",
                    data={
                        "next": "/twitch/datenschutz",
                        "cf-turnstile-response": "valid-token",
                    },
                    allow_redirects=False,
                )

        self.assertEqual(response.status, 302)
        self.assertEqual(response.headers.get("Location"), "/twitch/datenschutz")
        self.assertEqual(server_impl.last_verified_token, "valid-token")
        self.assertIn(LEGAL_GATE_COOKIE_NAME, response.cookies)

    async def test_valid_gate_cookie_allows_human_to_open_legal_page(self) -> None:
        server_impl = _DummyLegalServer(
            turnstile_site_key="site-key",
            turnstile_secret_key="secret-key",
            cookie_secret="cookie-secret",
        )
        app = web.Application()
        app.add_routes([web.get("/twitch/impressum", server_impl.abbo_impressum)])
        valid_cookie = server_impl._legal_gate_cookie_value(expires_at=2_000_000_000)

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                client.session.cookie_jar.update_cookies(
                    {LEGAL_GATE_COOKIE_NAME: valid_cookie}
                )
                response = await client.get(
                    "/twitch/impressum",
                    headers={"User-Agent": "Mozilla/5.0"},
                )
                body = await response.text()

        self.assertEqual(response.status, 200)
        self.assertIn("Impressum", body)

    async def test_verify_rejects_invalid_turnstile_response(self) -> None:
        server_impl = _DummyLegalServer(
            turnstile_site_key="site-key",
            turnstile_secret_key="secret-key",
            cookie_secret="cookie-secret",
            verify_ok=False,
        )
        app = web.Application()
        app.add_routes([web.post("/twitch/legal/verify", server_impl.legal_verify)])

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.post(
                    "/twitch/legal/verify",
                    data={
                        "next": "/twitch/impressum",
                        "cf-turnstile-response": "invalid-token",
                    },
                )
                body = await response.text()

        self.assertEqual(response.status, 403)
        self.assertEqual(body, "Turnstile verification failed.")

    async def test_turnstile_verification_requires_matching_hostname_and_action(self) -> None:
        server_impl = _DummyLegalServer(
            turnstile_site_key="site-key",
            turnstile_secret_key="secret-key",
            cookie_secret="cookie-secret",
        )
        fake_session = _FakeTurnstileSession(
            {
                "success": True,
                "action": "legal_access",
                "hostname": "twitch.earlysalty.com",
            }
        )
        aiohttp_request = type(
            "Req",
            (),
            {
                "headers": {"Host": "twitch.earlysalty.com"},
                "host": "twitch.earlysalty.com",
                "remote": "127.0.0.1",
            },
        )()

        with patch("bot.dashboard.admin.legal_mixin.aiohttp.ClientSession", return_value=fake_session):
            ok = await _DashboardLegalMixin._verify_legal_turnstile_token(
                server_impl,
                aiohttp_request,
                "token-1",
            )

        self.assertTrue(ok)
        self.assertEqual(fake_session.calls[0]["data"]["response"], "token-1")

        hostname_mismatch_session = _FakeTurnstileSession(
            {
                "success": True,
                "action": "legal_access",
                "hostname": "example.com",
            }
        )
        with patch(
            "bot.dashboard.admin.legal_mixin.aiohttp.ClientSession",
            return_value=hostname_mismatch_session,
        ):
            hostname_ok = await _DashboardLegalMixin._verify_legal_turnstile_token(
                server_impl,
                aiohttp_request,
                "token-2",
            )
        self.assertFalse(hostname_ok)

        missing_action_session = _FakeTurnstileSession(
            {
                "success": True,
                "hostname": "twitch.earlysalty.com",
            }
        )
        with patch(
            "bot.dashboard.admin.legal_mixin.aiohttp.ClientSession",
            return_value=missing_action_session,
        ):
            action_ok = await _DashboardLegalMixin._verify_legal_turnstile_token(
                server_impl,
                aiohttp_request,
                "token-3",
            )
        self.assertFalse(action_ok)

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
