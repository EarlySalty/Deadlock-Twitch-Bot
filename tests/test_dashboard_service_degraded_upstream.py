import html
import json
import unittest
from types import SimpleNamespace
from urllib.parse import parse_qs, urlencode, urlsplit
from unittest.mock import patch

from aiohttp import web

from bot import storage
from bot.core.constants import log
from bot.dashboard.live.live import DashboardLiveMixin
from bot.dashboard.route_deps import EntryRouteDeps
from bot.dashboard.routes_entry import discord_link as entry_discord_link
from bot.dashboard_service.app import build_dashboard_service_app
from bot.dashboard_service.client import BotApiClientError


def _query_params(location: str) -> dict[str, list[str]]:
    return parse_qs(urlsplit(location).query)


class _DummyLiveWriteHandler(DashboardLiveMixin):
    def __init__(self, *, payload: dict[str, str], upstream_error: BotApiClientError) -> None:
        self._payload = payload
        self._upstream_error = upstream_error

    def _require_token(self, request):
        del request

    async def _read_post_with_csrf(self, request, *, fallback_path: str = "/twitch/admin"):
        del request, fallback_path
        return self._payload

    def _redirect_location(
        self,
        request,
        *,
        ok: str | None = None,
        err: str | None = None,
        default_path: str = "/twitch/stats",
    ) -> str:
        del request, default_path
        params = {}
        if ok is not None:
            params["ok"] = ok
        if err is not None:
            params["err"] = err
        return f"/twitch?{urlencode(params)}" if params else "/twitch"

    def _safe_internal_redirect(self, location: str, *, fallback: str = "/twitch/stats") -> str:
        del fallback
        return location

    async def _do_add(self, raw: str) -> str:
        del raw
        raise self._upstream_error

    async def _remove(self, login: str) -> str:
        del login
        raise self._upstream_error

    async def _verify(self, login: str, mode: str) -> str:
        del login, mode
        raise self._upstream_error

    async def _archive(self, login: str, mode: str) -> str:
        del login, mode
        raise self._upstream_error

    async def _discord_flag(self, login: str, is_on_discord: bool) -> str:
        del login, is_on_discord
        raise self._upstream_error

    async def _discord_profile(
        self,
        login: str,
        discord_user_id: str | None,
        discord_display_name: str | None,
        mark_member: bool,
    ) -> str:
        del login, discord_user_id, discord_display_name, mark_member
        raise self._upstream_error


class _DummyEntryServer:
    def __init__(self, *, upstream_error: BotApiClientError) -> None:
        self._upstream_error = upstream_error

    def _require_token(self, request):
        del request

    def _csrf_verify_token(self, request, csrf_token: str) -> bool:
        del request, csrf_token
        return True

    def _redirect_location(
        self,
        request,
        *,
        ok: str | None = None,
        err: str | None = None,
        default_path: str = "/twitch/stats",
    ) -> str:
        del request, default_path
        params = {}
        if ok is not None:
            params["ok"] = ok
        if err is not None:
            params["err"] = err
        return f"/twitch?{urlencode(params)}" if params else "/twitch"

    def _safe_internal_redirect(self, location: str, *, fallback: str = "/twitch/stats") -> str:
        del fallback
        return location

    async def _discord_profile(
        self,
        login: str,
        *,
        discord_user_id: str | None,
        discord_display_name: str | None,
        mark_member: bool,
    ) -> str:
        del login, discord_user_id, discord_display_name, mark_member
        raise self._upstream_error


class _FakeDashboardApp:
    def __init__(self) -> None:
        self.store: dict[str, object] = {}
        self.on_startup: list[object] = []
        self.on_cleanup: list[object] = []

    def __setitem__(self, key, value):
        self.store[key] = value

    def __getitem__(self, key):
        return self.store[key]


class _UpstreamFailingBotApiClient:
    def __init__(self, **_kwargs) -> None:
        pass

    async def get_raid_go_url(self, state: str) -> str | None:
        del state
        raise BotApiClientError(
            status=503,
            code="upstream_unavailable",
            message="Bot internal API is unavailable.",
        )

    async def send_raid_requirements(self, login: str) -> str:
        del login
        raise BotApiClientError(
            status=503,
            code="upstream_unavailable",
            message="Bot internal API is unavailable.",
        )

    async def close(self) -> None:
        return None


class DashboardServiceDegradedUpstreamTests(unittest.IsolatedAsyncioTestCase):
    async def test_write_callbacks_raise_bot_api_error_when_internal_api_is_missing(self) -> None:
        captured: dict[str, object] = {}

        def _fake_build_v2_app(**kwargs):
            captured.update(kwargs)
            return _FakeDashboardApp()

        with patch("bot.dashboard_service.app.analytics_db_fingerprint_details", return_value={"fingerprint": "local", "hostHash": "h", "databaseHash": "d", "portHash": "p"}), patch("bot.dashboard_service.app.build_v2_app", side_effect=_fake_build_v2_app):
            build_dashboard_service_app(
                internal_api_base_url="http://127.0.0.1:1234",
                internal_api_token="",
                internal_api_allow_non_loopback=False,
                internal_api_timeout_seconds=1.0,
                dashboard_token="dash-token",
                partner_token="partner-token",
                noauth=False,
                oauth_client_id="client-id",
                oauth_client_secret="client-secret",
                oauth_redirect_uri="https://example.com/callback",
                session_ttl_seconds=3600,
                legacy_stats_url="https://example.com/stats",
            )

        services = captured["dashboard_services"]
        assert services is not None

        expectations = [
            (services.add_cb, ("partner_one", False)),
            (services.remove_cb, ("partner_one",)),
            (services.verify_cb, ("partner_one", "check")),
            (services.archive_cb, ("partner_one", "toggle")),
            (services.discord_flag_cb, ("partner_one", True)),
            (services.discord_profile_cb, ("partner_one", "123", "Partner One", True)),
        ]
        for callback, args in expectations:
            with self.subTest(callback=getattr(callback, "__name__", repr(callback))):
                with self.assertRaises(BotApiClientError) as ctx:
                    await callback(*args)
                self.assertEqual(ctx.exception.status, 503)
                self.assertEqual(ctx.exception.code, "upstream_unavailable")

    async def test_raid_callbacks_raise_bot_api_error_when_internal_api_is_missing(self) -> None:
        captured: dict[str, object] = {}

        def _fake_build_v2_app(**kwargs):
            captured.update(kwargs)
            return _FakeDashboardApp()

        with patch(
            "bot.dashboard_service.app.analytics_db_fingerprint_details",
            return_value={"fingerprint": "local", "hostHash": "h", "databaseHash": "d", "portHash": "p"},
        ), patch("bot.dashboard_service.app.build_v2_app", side_effect=_fake_build_v2_app):
            build_dashboard_service_app(
                internal_api_base_url="http://127.0.0.1:1234",
                internal_api_token="",
                internal_api_allow_non_loopback=False,
                internal_api_timeout_seconds=1.0,
                dashboard_token="dash-token",
                partner_token="partner-token",
                noauth=False,
                oauth_client_id="client-id",
                oauth_client_secret="client-secret",
                oauth_redirect_uri="https://example.com/callback",
                session_ttl_seconds=3600,
                legacy_stats_url="https://example.com/stats",
            )

        services = captured["dashboard_services"]
        assert services is not None

        expectations = [
            (services.raid_go_url_cb, ("state-token",)),
            (services.raid_requirements_cb, ("partner_one",)),
        ]
        for callback, args in expectations:
            with self.subTest(callback=getattr(callback, "__name__", repr(callback))):
                with self.assertRaises(BotApiClientError) as ctx:
                    await callback(*args)
                self.assertEqual(ctx.exception.status, 503)
                self.assertEqual(ctx.exception.code, "upstream_unavailable")

    async def test_raid_callbacks_raise_bot_api_error_when_upstream_client_fails(self) -> None:
        captured: dict[str, object] = {}

        def _fake_build_v2_app(**kwargs):
            captured.update(kwargs)
            return _FakeDashboardApp()

        with patch(
            "bot.dashboard_service.app.analytics_db_fingerprint_details",
            return_value={"fingerprint": "local", "hostHash": "h", "databaseHash": "d", "portHash": "p"},
        ), patch("bot.dashboard_service.app.build_v2_app", side_effect=_fake_build_v2_app), patch(
            "bot.dashboard_service.app.BotApiClient",
            _UpstreamFailingBotApiClient,
        ):
            build_dashboard_service_app(
                internal_api_base_url="http://127.0.0.1:1234",
                internal_api_token="internal-token",
                internal_api_allow_non_loopback=False,
                internal_api_timeout_seconds=1.0,
                dashboard_token="dash-token",
                partner_token="partner-token",
                noauth=False,
                oauth_client_id="client-id",
                oauth_client_secret="client-secret",
                oauth_redirect_uri="https://example.com/callback",
                session_ttl_seconds=3600,
                legacy_stats_url="https://example.com/stats",
            )

        services = captured["dashboard_services"]
        assert services is not None

        expectations = [
            (services.raid_go_url_cb, ("state-token",)),
            (services.raid_requirements_cb, ("partner_one",)),
        ]
        for callback, args in expectations:
            with self.subTest(callback=getattr(callback, "__name__", repr(callback))):
                with self.assertRaises(BotApiClientError) as ctx:
                    await callback(*args)
                self.assertEqual(ctx.exception.status, 503)
                self.assertEqual(ctx.exception.code, "upstream_unavailable")

    async def test_live_write_routes_redirect_to_err_on_bot_api_error(self) -> None:
        upstream_error = BotApiClientError(
            status=503,
            code="upstream_unavailable",
            message="Bot internal API is unavailable.",
        )
        cases = [
            ("add_any", {"q": "partner_one"}),
            ("add_url", {"url": "partner_one"}),
            ("add_login", {"login": "partner_one"}),
            ("add_streamer", {"login": "partner_one", "discord_user_id": "123", "discord_display_name": "Partner One", "member_flag": "on"}),
            ("discord_flag", {"login": "partner_one", "mode": "on"}),
            ("discord_link", {"login": "partner_one", "discord_user_id": "123", "discord_display_name": "Partner One", "member_flag": "on"}),
            ("remove", {"login": "partner_one"}),
            ("verify", {"login": "partner_one", "mode": "check"}),
            ("archive", {"login": "partner_one", "mode": "toggle"}),
        ]

        for method_name, payload in cases:
            handler = _DummyLiveWriteHandler(payload=payload, upstream_error=upstream_error)
            request = SimpleNamespace(match_info={})
            with self.subTest(method=method_name):
                with self.assertRaises(web.HTTPFound) as ctx:
                    await getattr(handler, method_name)(request)
                params = _query_params(ctx.exception.location)
                self.assertIn("err", params)
                self.assertNotIn("ok", params)

    async def test_entry_discord_link_redirects_to_err_on_bot_api_error(self) -> None:
        upstream_error = BotApiClientError(
            status=503,
            code="upstream_unavailable",
            message="Bot internal API is unavailable.",
        )
        server = _DummyEntryServer(upstream_error=upstream_error)
        request = SimpleNamespace(
            post=lambda: SimpleNamespace(),
            path="/twitch/discord_link",
        )

        async def _post():
            return {
                "csrf_token": "token",
                "login": "partner_one",
                "discord_user_id": "123",
                "discord_display_name": "Partner One",
                "member_flag": "on",
            }

        request.post = _post
        deps = EntryRouteDeps(
            critical_scopes=(),
            dashboard_v2_login_url="/twitch/auth/login",
            dashboards_discord_login_url="/twitch/auth/discord/login",
            dashboards_login_url="/twitch/auth/login",
            html=html,
            json=json,
            log=log,
            required_scopes=(),
            scope_column_labels={},
            storage=storage,
        )

        with self.assertRaises(web.HTTPFound) as ctx:
            await entry_discord_link(server, request, deps=deps)

        params = _query_params(ctx.exception.location)
        self.assertIn("err", params)
        self.assertNotIn("ok", params)


if __name__ == "__main__":
    unittest.main()
