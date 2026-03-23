from __future__ import annotations

import contextlib
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from aiohttp import web

from bot.dashboard.raids.raid_mixin import _DashboardRaidMixin


class _DummyRaidRequirementsRoute(_DashboardRaidMixin):
    def __init__(self) -> None:
        self.dashboard_session: dict[str, str] | None = None
        self.auth_level = "partner"
        self.require_token_calls = 0
        self._raid_bot = None

    def _get_dashboard_auth_session(self, _request):
        return self.dashboard_session

    def _get_auth_level(self, _request):
        return self.auth_level

    def _require_token(self, _request):
        self.require_token_calls += 1
        return None

    @staticmethod
    def _redirect_location(_request, *, ok: str, default_path: str) -> str:
        return f"{default_path}?ok={ok}"

    @staticmethod
    def _safe_internal_redirect(location: str, *, fallback: str) -> str:
        return location or fallback

    @staticmethod
    def _sanitize_log_value(value: str) -> str:
        return value


class DashboardRaidRequirementsAuthzTests(unittest.IsolatedAsyncioTestCase):
    async def test_non_admin_requires_active_partner_session(self) -> None:
        handler = _DummyRaidRequirementsRoute()
        handler.dashboard_session = {"twitch_login": "archivedstreamer"}
        request = SimpleNamespace(query={"login": "targetstreamer"})

        def _load_active_partner(_conn, *, twitch_login=None, twitch_user_id=None):
            del twitch_user_id
            if twitch_login == "targetstreamer":
                return {"twitch_login": "targetstreamer", "discord_user_id": "123"}
            return None

        with (
            patch(
                "bot.dashboard.raids.raid_mixin.storage.readonly_connection",
                side_effect=lambda: contextlib.nullcontext(object()),
            ),
            patch(
                "bot.dashboard.raids.raid_mixin.storage.load_active_partner",
                side_effect=_load_active_partner,
            ),
        ):
            response = await handler.raid_requirements(request)

        self.assertEqual(response.status, 403)
        self.assertEqual(response.text, "Dashboard streamer session required")

    async def test_non_admin_cannot_target_other_active_partner(self) -> None:
        handler = _DummyRaidRequirementsRoute()
        handler.dashboard_session = {"twitch_login": "ownstreamer"}
        request = SimpleNamespace(query={"login": "targetstreamer"})

        def _load_active_partner(_conn, *, twitch_login=None, twitch_user_id=None):
            del twitch_user_id
            if twitch_login == "targetstreamer":
                return {"twitch_login": "targetstreamer", "discord_user_id": "123"}
            if twitch_login == "ownstreamer":
                return {"twitch_login": "ownstreamer", "discord_user_id": "456"}
            return None

        with (
            patch(
                "bot.dashboard.raids.raid_mixin.storage.readonly_connection",
                side_effect=lambda: contextlib.nullcontext(object()),
            ),
            patch(
                "bot.dashboard.raids.raid_mixin.storage.load_active_partner",
                side_effect=_load_active_partner,
            ),
        ):
            response = await handler.raid_requirements(request)

        self.assertEqual(response.status, 403)
        self.assertEqual(response.text, "Forbidden streamer scope")

    async def test_admin_can_trigger_requirements_for_active_partner(self) -> None:
        handler = _DummyRaidRequirementsRoute()
        handler.auth_level = "admin"

        async def _requirements_cb(login: str) -> str:
            return f"sent:{login}"

        handler._raid_requirements_cb = _requirements_cb
        request = SimpleNamespace(query={"login": "targetstreamer"})

        with (
            patch(
                "bot.dashboard.raids.raid_mixin.storage.readonly_connection",
                side_effect=lambda: contextlib.nullcontext(object()),
            ),
            patch(
                "bot.dashboard.raids.raid_mixin.storage.load_active_partner",
                side_effect=lambda _conn, *, twitch_login=None, twitch_user_id=None: (
                    {"twitch_login": twitch_login, "discord_user_id": "123"}
                    if twitch_login == "targetstreamer"
                    else None
                ),
            ),
        ):
            with self.assertRaises(web.HTTPFound) as ctx:
                await handler.raid_requirements(request)

        self.assertEqual(ctx.exception.location, "/twitch/admin?ok=sent:targetstreamer")


if __name__ == "__main__":
    unittest.main()
