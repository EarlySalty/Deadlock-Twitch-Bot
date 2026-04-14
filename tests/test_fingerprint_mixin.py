"""Tests for the post-login JS fingerprint collection flow."""

from __future__ import annotations

import hashlib
import unittest

from aiohttp import web
from aiohttp.test_utils import TestClient, TestServer, make_mocked_request


class FakeRepo:
    def __init__(self) -> None:
        self.saved: list[dict] = []

    def save_discord_admin_session(self, **kwargs) -> None:
        self.saved.append(kwargs)


class FakeCache:
    def __init__(self) -> None:
        self.saved: list[tuple[str, dict]] = []

    def put(self, key: str, payload: dict) -> dict:
        record = dict(payload)
        self.saved.append((key, record))
        return record


class FakeServer:
    _discord_admin_cookie_name = "twitch_dash_session"
    _session_cookie_name = "twitch_dash_session"

    def __init__(self) -> None:
        self._fake_session: dict = {}
        self.repo = FakeRepo()
        self.cache = FakeCache()

    def _get_discord_admin_session(self, request):
        return self._fake_session or None

    def _dashboard_auth_state_repo(self):
        return self.repo

    def _dashboard_auth_state_cache(self, attr_name):
        if attr_name != "_discord_admin_sessions":
            raise AssertionError(f"unexpected cache namespace: {attr_name}")
        return self.cache

    def _safe_internal_redirect(self, location, *, fallback="/twitch/admin"):
        candidate = str(location or "").strip()
        if not candidate or not candidate.startswith("/") or candidate.startswith("//"):
            return fallback
        return candidate

    async def fingerprint_page(self, request):
        from bot.dashboard.auth.fingerprint_mixin import fingerprint_page

        return await fingerprint_page(self, request)

    async def fingerprint_submit(self, request):
        from bot.dashboard.auth.fingerprint_mixin import fingerprint_submit

        return await fingerprint_submit(self, request)


class FingerprintMixinTests(unittest.IsolatedAsyncioTestCase):
    async def test_fingerprint_page_returns_html_for_authenticated_session(self):
        server = FakeServer()
        server._fake_session = {"fp_pending": True}
        request = make_mocked_request(
            "GET",
            "/twitch/auth/fingerprint",
            headers={"Cookie": "twitch_dash_session=abc123"},
        )

        response = await server.fingerprint_page(request)

        self.assertEqual(response.status, 200)
        self.assertTrue(
            "fingerprint" in response.text.lower() or "canvas" in response.text.lower()
        )

    async def test_fingerprint_submit_stores_fp_and_clears_pending_flag(self):
        server = FakeServer()
        session = {
            "fp_pending": True,
            "created_at": 100.0,
            "expires_at": 200.0,
            "post_fp_destination": "/twitch/admin",
        }
        server._fake_session = session
        fp_value = hashlib.sha256(b"canvas-data-xyz").hexdigest()[:32]

        app = web.Application()
        app.router.add_post("/twitch/auth/fingerprint", server.fingerprint_submit)

        async with TestClient(TestServer(app)) as client:
            resp = await client.post(
                "/twitch/auth/fingerprint",
                data={"fp": fp_value},
                headers={"Cookie": "twitch_dash_session=abc123"},
                allow_redirects=False,
            )

        self.assertEqual(resp.status, 303)
        self.assertEqual(resp.headers["Location"], "/twitch/admin")
        self.assertEqual(session["js_fp"], fp_value)
        self.assertIs(session["fp_pending"], False)
        self.assertTrue(server.repo.saved)
        self.assertTrue(server.cache.saved)
        self.assertEqual(server.cache.saved[-1][0], "abc123")
        self.assertEqual(server.cache.saved[-1][1]["js_fp"], fp_value)

    async def test_fingerprint_submit_rejects_too_short_fp(self):
        server = FakeServer()
        session = {"fp_pending": True, "post_fp_destination": "/twitch/admin"}
        server._fake_session = session

        app = web.Application()
        app.router.add_post("/twitch/auth/fingerprint", server.fingerprint_submit)

        async with TestClient(TestServer(app)) as client:
            resp = await client.post(
                "/twitch/auth/fingerprint",
                data={"fp": "abc123"},
                headers={"Cookie": "twitch_dash_session=abc123"},
                allow_redirects=False,
            )

        self.assertEqual(resp.status, 303)
        self.assertEqual(resp.headers["Location"], "/twitch/admin")
        self.assertEqual(session["js_fp"], hashlib.sha256(b"fallback").hexdigest()[:32])

    async def test_fingerprint_submit_revalidates_redirect_destination(self):
        server = FakeServer()
        session = {"fp_pending": True, "post_fp_destination": "//evil.example"}
        server._fake_session = session

        app = web.Application()
        app.router.add_post("/twitch/auth/fingerprint", server.fingerprint_submit)

        async with TestClient(TestServer(app)) as client:
            resp = await client.post(
                "/twitch/auth/fingerprint",
                data={"fp": hashlib.sha256(b"canvas-data-xyz").hexdigest()[:32]},
                headers={"Cookie": "twitch_dash_session=abc123"},
                allow_redirects=False,
            )

        self.assertEqual(resp.status, 303)
        self.assertEqual(resp.headers["Location"], "/twitch/admin")
