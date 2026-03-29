import unittest
from unittest.mock import patch

from aiohttp import web

from bot.dashboard.routes_mixin import _DashboardRoutesMixin


class _DummyDashboardRoutes(_DashboardRoutesMixin):
    def __init__(self, clip_manager) -> None:
        self._social_media_clip_manager = None
        self._raid_bot = type(
            "RaidBotStub",
            (),
            {
                "_cog": type(
                    "CogStub",
                    (),
                    {
                        "clip_manager": clip_manager,
                        "api": getattr(clip_manager, "twitch_api", None),
                    },
                )()
            },
        )()

    def _check_v2_auth(self, request):
        del request
        return True

    def _get_dashboard_auth_session(self, request):
        del request
        return {}

    def _get_auth_level(self, request):
        del request
        return "admin"

    def _billing_configured_public_origin(self) -> str:
        return "https://example.com"


class DashboardRoutesSocialMediaTests(unittest.TestCase):
    def test_register_social_media_routes_reuses_primary_clip_manager(self) -> None:
        existing_manager = type("ClipManagerStub", (), {"twitch_api": object()})()
        handler = _DummyDashboardRoutes(existing_manager)
        app = web.Application()

        with patch("bot.social_media.create_social_media_app", return_value=web.Application()) as create_app:
            handler._register_social_media_routes(app)

        self.assertIs(handler._social_media_clip_manager, existing_manager)
        self.assertIs(create_app.call_args.kwargs["clip_manager"], existing_manager)


if __name__ == "__main__":
    unittest.main()
