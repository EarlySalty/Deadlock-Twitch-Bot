import unittest

from aiohttp import web

from bot.dashboard.routes_title import _resolve_effective_twitch_login


class _DummyTitleServer:
    def __init__(self, *, session: dict[str, object] | None, auth_level: str) -> None:
        self._session = session
        self._auth_level = auth_level

    def _get_dashboard_session(self, request):
        del request
        return self._session

    def _get_auth_level(self, request):
        del request
        return self._auth_level


class DashboardTitleRouteScopeTests(unittest.TestCase):
    def test_partner_cannot_override_streamer(self) -> None:
        server = _DummyTitleServer(
            session={"twitch_login": "earlysalty", "twitch_user_id": "123"},
            auth_level="partner",
        )

        with self.assertRaises(web.HTTPForbidden):
            _resolve_effective_twitch_login(
                server,
                object(),
                "https://www.twitch.tv/denoshock",
            )

    def test_partner_uses_own_session_login_without_override(self) -> None:
        server = _DummyTitleServer(
            session={"twitch_login": "earlysalty", "twitch_user_id": "123"},
            auth_level="partner",
        )

        self.assertEqual(
            _resolve_effective_twitch_login(server, object(), None),
            "earlysalty",
        )

    def test_admin_can_override_streamer_with_twitch_url(self) -> None:
        server = _DummyTitleServer(
            session={"twitch_login": "earlysalty", "twitch_user_id": "123"},
            auth_level="admin",
        )

        self.assertEqual(
            _resolve_effective_twitch_login(
                server,
                object(),
                "https://www.twitch.tv/denoshock",
            ),
            "denoshock",
        )


if __name__ == "__main__":
    unittest.main()
