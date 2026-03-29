from __future__ import annotations

import json
import subprocess
import sys
import unittest
from pathlib import Path
from unittest.mock import patch

from aiohttp import web

from bot.app_keys import BOT_API_CLIENT_KEY
from bot.dashboard_service.app import build_dashboard_service_app
from bot.runtime.dashboard_runtime import DashboardRuntimeServices


REPO_ROOT = Path(__file__).resolve().parents[1]


class _FakeBotApiClient:
    def __init__(self) -> None:
        self.calls: list[tuple[str, tuple[object, ...], dict[str, object]]] = []
        self.closed = False

    async def add_streamer(self, login: str, require_link: bool) -> str:
        self.calls.append(("add_streamer", (login, require_link), {}))
        return "added"

    async def remove_streamer(self, login: str) -> str:
        self.calls.append(("remove_streamer", (login,), {}))
        return "removed"

    async def get_streamers(self) -> list[dict[str, object]]:
        self.calls.append(("get_streamers", tuple(), {}))
        return []

    async def get_stats(self, **kwargs: object) -> dict[str, object]:
        self.calls.append(("get_stats", tuple(), dict(kwargs)))
        return {"ok": True}

    async def verify_streamer(self, login: str, mode: str) -> str:
        self.calls.append(("verify_streamer", (login, mode), {}))
        return "verified"

    async def archive_streamer(self, login: str, mode: str) -> str:
        self.calls.append(("archive_streamer", (login, mode), {}))
        return "archived"

    async def set_discord_flag(self, login: str, is_on_discord: bool) -> str:
        self.calls.append(("set_discord_flag", (login, is_on_discord), {}))
        return "discord-flag-updated"

    async def save_discord_profile(
        self,
        login: str,
        *,
        discord_user_id: str | None,
        discord_display_name: str | None,
        mark_member: bool,
    ) -> str:
        self.calls.append(
            (
                "save_discord_profile",
                (login,),
                {
                    "discord_user_id": discord_user_id,
                    "discord_display_name": discord_display_name,
                    "mark_member": mark_member,
                },
            )
        )
        return "profile-updated"

    async def get_raid_auth_url(
        self,
        login: str,
        *,
        discord_user_id: str | None = None,
        scope_profile: str | None = None,
    ) -> str:
        self.calls.append(
            (
                "get_raid_auth_url",
                (login,),
                {
                    "discord_user_id": discord_user_id,
                    "scope_profile": scope_profile,
                },
            )
        )
        return "https://auth.example/raid"

    async def get_raid_go_url(self, state: str) -> str | None:
        self.calls.append(("get_raid_go_url", (state,), {}))
        return "https://raid.example/go"

    async def send_raid_requirements(self, login: str) -> str:
        self.calls.append(("send_raid_requirements", (login,), {}))
        return "requirements-sent"

    async def process_raid_oauth_callback(
        self,
        *,
        code: str,
        state: str,
        error: str,
    ) -> dict[str, object]:
        self.calls.append(
            ("process_raid_oauth_callback", tuple(), {"code": code, "state": state, "error": error})
        )
        return {"status": 200, "ok": True}

    async def healthz(self) -> dict[str, object]:
        self.calls.append(("healthz", tuple(), {}))
        return {"ok": True, "analyticsDbFingerprint": "pg:local"}

    async def close(self) -> None:
        self.closed = True


class SplitRuntimeImportBoundaryTests(unittest.TestCase):
    def _run_import(self, statement: str) -> dict[str, list[str]]:
        code = (
            "import json\n"
            "import sys\n\n"
            "before = set(sys.modules)\n"
            f"{statement.rstrip()}\n"
            "loaded = sorted(name for name in sys.modules if name not in before)\n"
            "print(json.dumps({\n"
            '    "all": loaded,\n'
            '    "bot_service": [name for name in loaded if name.startswith("bot.bot_service")],\n'
            '    "dashboard_service": [name for name in loaded if name.startswith("bot.dashboard_service")],\n'
            '    "cog": [name for name in loaded if name == "bot.cog"],\n'
            "}))\n"
        )
        completed = subprocess.run(
            [sys.executable, "-c", code],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=True,
        )
        return json.loads(completed.stdout)

    def test_importing_bot_service_app_does_not_pull_dashboard_service_or_cog(self) -> None:
        loaded = self._run_import("import bot.bot_service.app")

        self.assertEqual(loaded["bot_service"], ["bot.bot_service", "bot.bot_service.app"])
        self.assertEqual(loaded["dashboard_service"], [])
        self.assertEqual(loaded["cog"], [])

    def test_importing_dashboard_service_app_does_not_pull_bot_service_or_cog(self) -> None:
        loaded = self._run_import("import bot.dashboard_service.app")

        self.assertIn("bot.dashboard_service", loaded["dashboard_service"])
        self.assertIn("bot.dashboard_service.app", loaded["dashboard_service"])
        self.assertEqual(loaded["bot_service"], [])
        self.assertEqual(loaded["cog"], [])


class SplitRuntimeWiringTests(unittest.IsolatedAsyncioTestCase):
    async def test_dashboard_service_app_wires_bot_api_client_without_raid_bot(self) -> None:
        fake_client = _FakeBotApiClient()
        captured_kwargs: dict[str, object] = {}

        def _fake_build_v2_app(**kwargs):
            captured_kwargs.update(kwargs)
            return web.Application()

        with (
            patch.dict(
                "os.environ",
                {
                    "TWITCH_ALLOW_DASHBOARD_NOAUTH": "1",
                    "TWITCH_DASHBOARD_NOAUTH": "1",
                    "TWITCH_DASHBOARD_HOST": "127.0.0.1",
                },
                clear=False,
            ),
            patch(
                "bot.dashboard_service.app.analytics_db_fingerprint_details",
                return_value={"fingerprint": "pg:local"},
            ),
            patch("bot.dashboard_service.app.BotApiClient", return_value=fake_client),
            patch("bot.dashboard_service.app.build_v2_app", side_effect=_fake_build_v2_app),
        ):
            app = build_dashboard_service_app(
                noauth=True,
                dashboard_token="",
                partner_token="",
                oauth_client_id="client-id",
                oauth_client_secret="client-secret",
                internal_api_token="secret",
                internal_api_base_url="http://127.0.0.1:8776",
                session_ttl_seconds=6 * 3600,
                legacy_stats_url="",
            )

        self.assertIsInstance(app, web.Application)
        self.assertIs(app[BOT_API_CLIENT_KEY], fake_client)
        self.assertNotIn("raid_bot", captured_kwargs)

        services = captured_kwargs["dashboard_services"]
        self.assertIsInstance(services, DashboardRuntimeServices)

        add_cb = services.add_cb
        remove_cb = services.remove_cb
        raid_auth_url_cb = services.raid_auth_url_cb
        raid_oauth_callback_cb = services.raid_oauth_callback_cb
        self.assertTrue(callable(add_cb))
        self.assertTrue(callable(remove_cb))
        self.assertTrue(callable(raid_auth_url_cb))
        self.assertTrue(callable(raid_oauth_callback_cb))

        self.assertEqual(await add_cb("Early_Salty", True), "added")
        self.assertEqual(await remove_cb("Early_Salty"), "removed")
        self.assertEqual(await raid_auth_url_cb("Early_Salty"), "https://auth.example/raid")
        self.assertEqual(
            await raid_oauth_callback_cb(code="oauth-code", state="state", error=""),
            {"status": 200, "ok": True},
        )


if __name__ == "__main__":
    unittest.main()
