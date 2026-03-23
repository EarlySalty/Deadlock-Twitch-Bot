from __future__ import annotations

import sqlite3
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from aiohttp import web

from bot.dashboard.announcement_mode_mixin import DashboardAdminAnnouncementMixin
from bot.dashboard.server_v2 import DashboardV2Server
from bot.dashboard.templates import DashboardTemplateMixin
from bot.promo_mode import load_global_promo_mode, save_global_promo_mode


class _FakeRequest:
    def __init__(
        self,
        *,
        method: str = "GET",
        path: str = "/twitch/admin/announcements",
        host: str = "admin.earlysalty.de",
        query: dict | None = None,
        body: dict | None = None,
    ) -> None:
        self.method = method
        self.path = path
        self.host = host
        self.query = query or {}
        self.headers = {"Host": host}
        self.rel_url = SimpleNamespace(path_qs=path)
        self._body = body or {}

    async def post(self):
        return self._body


class _DummyAnnouncementPage(DashboardAdminAnnouncementMixin, DashboardTemplateMixin):
    def __init__(self) -> None:
        self.authenticated = True
        self.valid_csrf = "csrf-ok"
        self.discord_admin_session = {"user_id": "55"}

    def _is_local_request(self, _request):
        return False

    def _is_admin_dashboard_host_request(self, request):
        return str(request.host or "").strip().lower() == "admin.earlysalty.de"

    def _require_token(self, _request):
        if not self.authenticated:
            raise web.HTTPUnauthorized(text="discord admin required")
        return None

    def _csrf_generate_token(self, _request):
        return self.valid_csrf

    def _csrf_verify_token(self, _request, token: str) -> bool:
        return token == self.valid_csrf

    def _get_discord_admin_session(self, _request):
        return self.discord_admin_session

    def _redirect_location(
        self,
        _request,
        *,
        ok=None,
        err=None,
        default_path="/twitch/admin/announcements",
    ):
        if err:
            return f"{default_path}?err=1"
        if ok:
            return f"{default_path}?ok=1"
        return default_path


class _ConnCtx:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def __enter__(self) -> sqlite3.Connection:
        return self._conn

    def __exit__(self, exc_type, exc, tb) -> bool:
        return False


class DashboardAdminAnnouncementModeTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self.handler = _DummyAnnouncementPage()
        self.conn = sqlite3.connect(":memory:")
        self.conn.row_factory = sqlite3.Row

    def tearDown(self) -> None:
        self.conn.close()

    def _storage_patch(self):
        return patch(
            "bot.dashboard.admin.announcement_mode_mixin._storage.readonly_connection",
            return_value=_ConnCtx(self.conn),
        ), patch(
            "bot.dashboard.admin.announcement_mode_mixin._storage.transaction",
            return_value=_ConnCtx(self.conn),
        )

    async def test_admin_page_renders_saved_values_and_status(self) -> None:
        with self._storage_patch()[0], self._storage_patch()[1]:
            save_global_promo_mode(
                self.conn,
                config={
                    "mode": "custom_event",
                    "custom_message": "Event live",
                    "is_enabled": True,
                },
                updated_by="discord:55",
            )
            response = await self.handler.admin_announcements_page(_FakeRequest())

        self.assertEqual(response.status, 200)
        self.assertIn("Globaler Announcement-Modus", response.text)
        self.assertIn("Event live", response.text)
        self.assertIn("Aktiv", response.text)

    async def test_admin_can_save_and_reload_configuration(self) -> None:
        request = _FakeRequest(
            method="POST",
            body={
                "csrf_token": "csrf-ok",
                "mode": "custom_event",
                "custom_message": "Turnier heute",
                "is_enabled": "1",
                "starts_at": "2026-03-07T18:00",
                "ends_at": "2026-03-07T22:00",
            },
        )

        with self._storage_patch()[0], self._storage_patch()[1]:
            with self.assertRaises(web.HTTPFound) as ctx:
                await self.handler.admin_announcements_save(request)
            saved = load_global_promo_mode(self.conn)

        self.assertEqual(ctx.exception.location, "/twitch/admin/announcements?ok=1")
        self.assertEqual(saved["mode"], "custom_event")
        self.assertEqual(saved["custom_message"], "Turnier heute")
        self.assertTrue(saved["is_enabled"])
        self.assertEqual(saved["starts_at"], "2026-03-07T18:00:00+00:00")
        self.assertEqual(saved["ends_at"], "2026-03-07T22:00:00+00:00")

    async def test_invalid_message_is_rejected(self) -> None:
        request = _FakeRequest(
            method="POST",
            body={
                "csrf_token": "csrf-ok",
                "mode": "custom_event",
                "custom_message": "Falscher Platzhalter {channel}",
                "is_enabled": "1",
            },
        )

        with self._storage_patch()[0], self._storage_patch()[1]:
            with self.assertRaises(web.HTTPFound) as ctx:
                await self.handler.admin_announcements_save(request)

        self.assertEqual(ctx.exception.location, "/twitch/admin/announcements?err=1")

    async def test_non_admin_host_redirects_to_canonical_admin_domain(self) -> None:
        with self._storage_patch()[0], self._storage_patch()[1]:
            with self.assertRaises(web.HTTPFound) as ctx:
                await self.handler.admin_announcements_page(
                    _FakeRequest(host="twitch.earlysalty.com")
                )

        self.assertEqual(
            ctx.exception.location,
            "https://admin.earlysalty.de/twitch/admin/announcements",
        )

    async def test_non_admin_request_is_blocked(self) -> None:
        self.handler.authenticated = False

        with self._storage_patch()[0], self._storage_patch()[1]:
            with self.assertRaises(web.HTTPUnauthorized):
                await self.handler.admin_announcements_page(_FakeRequest())

    def test_discord_admin_post_login_keeps_announcements_path(self) -> None:
        self.assertEqual(
            DashboardV2Server._canonical_discord_admin_post_login_path(
                "/twitch/admin/announcements"
            ),
            "/twitch/admin/announcements",
        )


if __name__ == "__main__":
    unittest.main()
