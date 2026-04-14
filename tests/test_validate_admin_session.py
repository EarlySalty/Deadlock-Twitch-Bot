"""Tests for /twitch/auth/validate shared admin cookie validation."""

from __future__ import annotations

import hashlib
import unittest

from aiohttp.test_utils import make_mocked_request


def _passive_fp_hash(ua: str, lang: str = "", platform: str = "") -> str:
    return hashlib.sha256(f"{ua}|{lang}|{platform}".encode("utf-8")).hexdigest()[:32]


class FakeServer:
    _discord_admin_cookie_name = "twitch_dash_session"
    _session_cookie_name = "twitch_dash_session"
    _discord_admin_session_ttl = 24 * 3600
    _fake_session: dict = {}

    def _is_local_request(self, request):
        return getattr(request, "_fake_local_request", False)

    def _get_discord_admin_session(self, request):
        return self._fake_session or None

    def _get_dashboard_auth_session(self, request):
        return None

    def _peer_host(self, request):
        return getattr(request, "_fake_peer", "127.0.0.1")

    def _is_trusted_proxy_host(self, raw):
        return self._host_without_port(raw) in {"127.0.0.1", "::1", "localhost"}

    def _forwarded_client_host(self, request):
        return getattr(request, "_fake_forwarded_client_ip", "")

    def _host_without_port(self, raw):
        value = str(raw or "").strip()
        if not value:
            return ""
        return value.split(":", 1)[0].lower()

    def _effective_client_host(self, request, peer):
        return getattr(request, "_fake_client_ip", peer)

    def _is_secure_request(self, request):
        return True

    def _sanitize_log_value(self, value):
        return str(value)

    def _set_discord_admin_cookie(self, response, request, session_id):
        response.set_cookie(
            self._discord_admin_cookie_name,
            session_id,
            max_age=self._discord_admin_session_ttl,
            httponly=True,
            secure=True,
            samesite="Lax",
            path="/",
        )

    async def validate_admin_session(self, request):
        from bot.dashboard.server_v2 import DashboardV2Server

        return await DashboardV2Server.validate_admin_session(self, request)


class ValidateAdminSessionTests(unittest.IsolatedAsyncioTestCase):
    async def test_validate_returns_401_without_admin_session(self):
        server = FakeServer()
        server._fake_session = {}
        request = make_mocked_request("GET", "/twitch/auth/validate")

        response = await server.validate_admin_session(request)

        self.assertEqual(response.status, 401)

    async def test_validate_accepts_localhost_without_cookie(self):
        server = FakeServer()
        request = make_mocked_request("GET", "/twitch/auth/validate")
        request._fake_local_request = True

        response = await server.validate_admin_session(request)

        self.assertEqual(response.status, 200)

    async def test_validate_rejects_wrong_ip(self):
        server = FakeServer()
        server._fake_session = {
            "client_ip": "1.2.3.4",
            "passive_fp": _passive_fp_hash("Mozilla/5.0", "de-DE", "macOS"),
            "js_fp": "abc12345",
            "fp_pending": False,
        }
        request = make_mocked_request(
            "GET",
            "/twitch/auth/validate",
            headers={
                "Cookie": "twitch_dash_session=abc123",
                "User-Agent": "Mozilla/5.0",
                "Accept-Language": "de-DE",
                "Sec-CH-UA-Platform": '"macOS"',
            },
        )
        request._fake_forwarded_client_ip = "5.6.7.8"

        response = await server.validate_admin_session(request)

        self.assertEqual(response.status, 401)

    async def test_validate_skips_ip_binding_without_forwarded_client_ip_from_trusted_proxy(self):
        server = FakeServer()
        server._fake_session = {
            "client_ip": "1.2.3.4",
            "passive_fp": _passive_fp_hash("Mozilla/5.0", "de-DE", "macOS"),
            "js_fp": "abc12345",
            "fp_pending": False,
            "username": "validated-admin",
        }
        request = make_mocked_request(
            "GET",
            "/twitch/auth/validate",
            headers={
                "Cookie": "twitch_dash_session=abc123",
                "User-Agent": "Mozilla/5.0",
                "Accept-Language": "de-DE",
                "Sec-CH-UA-Platform": '"macOS"',
            },
        )

        response = await server.validate_admin_session(request)

        self.assertEqual(response.status, 200)

    async def test_validate_rejects_wrong_passive_fp(self):
        server = FakeServer()
        server._fake_session = {
            "client_ip": "10.0.0.1",
            "passive_fp": _passive_fp_hash("Mozilla/5.0 (Linux)", "de-DE", "Linux"),
            "js_fp": "abc12345",
            "fp_pending": False,
        }
        request = make_mocked_request(
            "GET",
            "/twitch/auth/validate",
            headers={
                "Cookie": "twitch_dash_session=abc123",
                "User-Agent": "Mozilla/5.0 (Macintosh)",
                "Accept-Language": "de-DE",
                "Sec-CH-UA-Platform": '"macOS"',
            },
        )
        request._fake_client_ip = "10.0.0.1"

        response = await server.validate_admin_session(request)

        self.assertEqual(response.status, 401)

    async def test_validate_rejects_pending_fingerprint_session(self):
        server = FakeServer()
        server._fake_session = {
            "client_ip": "10.0.0.1",
            "passive_fp": _passive_fp_hash("Mozilla/5.0", "de-DE", "macOS"),
            "fp_pending": True,
            "js_fp": "",
        }
        request = make_mocked_request(
            "GET",
            "/twitch/auth/validate",
            headers={
                "Cookie": "twitch_dash_session=abc123",
                "User-Agent": "Mozilla/5.0",
                "Accept-Language": "de-DE",
                "Sec-CH-UA-Platform": '"macOS"',
            },
        )
        request._fake_client_ip = "10.0.0.1"

        response = await server.validate_admin_session(request)

        self.assertEqual(response.status, 401)

    async def test_validate_accepts_correct_ip_and_fingerprints_and_refreshes_cookie(self):
        server = FakeServer()
        server._fake_session = {
            "client_ip": "10.0.0.1",
            "passive_fp": _passive_fp_hash("Mozilla/5.0 (Macintosh)", "de-DE", "macOS"),
            "js_fp": "deadbeefcafebabe",
            "fp_pending": False,
            "username": "validated-admin",
        }
        request = make_mocked_request(
            "GET",
            "/twitch/auth/validate",
            headers={
                "Cookie": "twitch_dash_session=abc123",
                "User-Agent": "Mozilla/5.0 (Macintosh)",
                "Accept-Language": "de-DE,de;q=0.9",
                "Sec-CH-UA-Platform": '"macOS"',
            },
        )
        request._fake_forwarded_client_ip = "10.0.0.1"

        response = await server.validate_admin_session(request)

        self.assertEqual(response.status, 200)
        self.assertEqual(response.headers["X-Admin-User"], "validated-admin")
        cookie = response.cookies.get("twitch_dash_session")
        self.assertIsNotNone(cookie)
        self.assertEqual(cookie["max-age"], str(24 * 3600))
