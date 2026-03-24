import asyncio
import unittest
from unittest.mock import patch

from aiohttp import web

from bot.app_keys import (
    ANALYTICS_DB_FINGERPRINT_KEY,
    ANALYTICS_DB_FINGERPRINT_MISMATCH_KEY,
    BOT_API_CLIENT_KEY,
    INTERNAL_API_ANALYTICS_DB_FINGERPRINT_KEY,
)
from bot.dashboard_service.app import build_dashboard_service_app
from bot.dashboard_service.client import BotApiClient, BotApiClientError
from bot.internal_api import INTERNAL_API_BASE_PATH


class _FakeResponse:
    def __init__(self, *, status: int, text: str) -> None:
        self.status = int(status)
        self._text = text
        self.released = False

    async def text(self) -> str:
        return self._text

    def release(self) -> None:
        self.released = True


class _FakeSession:
    def __init__(
        self, *, response: _FakeResponse | None = None, exc: Exception | None = None
    ) -> None:
        self._response = response
        self._exc = exc
        self.closed = False
        self.calls: list[dict] = []

    async def request(self, method: str, url: str, **kwargs):
        self.calls.append({"method": method, "url": url, "kwargs": kwargs})
        if self._exc is not None:
            raise self._exc
        return self._response

    async def close(self) -> None:
        self.closed = True


class BotApiClientErrorMappingTests(unittest.IsolatedAsyncioTestCase):
    async def test_maps_upstream_auth_failures_to_safe_bad_gateway(self) -> None:
        session = _FakeSession(
            response=_FakeResponse(status=401, text='{"error":"missing token"}')
        )
        client = BotApiClient(
            base_url="http://127.0.0.1:8766",
            token="secret",
            session=session,
        )

        with self.assertRaises(BotApiClientError) as ctx:
            await client.get_streamers()

        self.assertEqual(ctx.exception.status, 502)
        self.assertEqual(ctx.exception.code, "upstream_auth_failed")
        self.assertIn("authenticate", ctx.exception.message.lower())

    async def test_maps_timeout_to_gateway_timeout(self) -> None:
        session = _FakeSession(exc=asyncio.TimeoutError())
        client = BotApiClient(
            base_url="http://127.0.0.1:8766",
            token="secret",
            session=session,
        )

        with self.assertRaises(BotApiClientError) as ctx:
            await client.get_streamers()

        self.assertEqual(ctx.exception.status, 504)
        self.assertEqual(ctx.exception.code, "upstream_timeout")

    async def test_invalid_json_success_response_maps_to_safe_error(self) -> None:
        response = _FakeResponse(status=200, text="<html>not json</html>")
        session = _FakeSession(response=response)
        client = BotApiClient(
            base_url="http://127.0.0.1:8766",
            token="secret",
            session=session,
        )

        with self.assertRaises(BotApiClientError) as ctx:
            await client.healthz()

        self.assertEqual(ctx.exception.status, 502)
        self.assertEqual(ctx.exception.code, "upstream_invalid_json")
        self.assertTrue(response.released)

    async def test_bad_request_message_is_sanitized(self) -> None:
        session = _FakeSession(
            response=_FakeResponse(status=400, text='{"message":"bad\\ninput"}')
        )
        client = BotApiClient(
            base_url="http://127.0.0.1:8766",
            token="secret",
            session=session,
        )

        with self.assertRaises(BotApiClientError) as ctx:
            await client.add_streamer("bad_login", require_link=False)

        self.assertEqual(ctx.exception.status, 400)
        self.assertEqual(ctx.exception.code, "bad_request")
        self.assertNotIn("\n", ctx.exception.message)

    async def test_base_url_with_internal_prefix_is_not_duplicated(self) -> None:
        session = _FakeSession(response=_FakeResponse(status=200, text="[]"))
        client = BotApiClient(
            base_url=f"http://127.0.0.1:8766{INTERNAL_API_BASE_PATH}",
            token="secret",
            session=session,
        )

        payload = await client.get_streamers()

        self.assertEqual(payload, [])
        self.assertEqual(len(session.calls), 1)
        self.assertEqual(
            session.calls[0]["url"],
            "http://127.0.0.1:8766/internal/twitch/v1/streamers",
        )

    async def test_rejects_path_confusing_logins_for_streamer_path_segments(
        self,
    ) -> None:
        session = _FakeSession(
            response=_FakeResponse(status=200, text='{"message":"removed"}')
        )
        client = BotApiClient(
            base_url="http://127.0.0.1:8766",
            token="secret",
            session=session,
        )

        for login in ("..", "foo/bar", "foo%2fbar", "foo%5cbar", "ab"):
            with self.subTest(login=login):
                with self.assertRaises(BotApiClientError) as ctx:
                    await client.remove_streamer(login)
                self.assertEqual(ctx.exception.status, 400)
                self.assertEqual(ctx.exception.code, "bad_request")

        self.assertEqual(session.calls, [])

    async def test_normalizes_valid_login_before_using_path_segment(self) -> None:
        session = _FakeSession(
            response=_FakeResponse(status=200, text='{"message":"verified"}')
        )
        client = BotApiClient(
            base_url="http://127.0.0.1:8766",
            token="secret",
            session=session,
        )

        message = await client.verify_streamer("Early_Salty", mode="manual")

        self.assertEqual(message, "verified")
        self.assertEqual(len(session.calls), 1)
        self.assertEqual(
            session.calls[0]["url"],
            "http://127.0.0.1:8766/internal/twitch/v1/streamers/early_salty/verify",
        )

    async def test_get_raid_auth_state_calls_internal_endpoint(self) -> None:
        session = _FakeSession(
            response=_FakeResponse(
                status=200,
                text='{"ok":true,"discord_user_id":"123","authorized":true}',
            )
        )
        client = BotApiClient(
            base_url="http://127.0.0.1:8766",
            token="secret",
            session=session,
        )

        payload = await client.get_raid_auth_state(discord_user_id="123")

        self.assertTrue(payload["authorized"])
        self.assertEqual(
            session.calls[0]["url"],
            "http://127.0.0.1:8766/internal/twitch/v1/raid/auth-state?discord_user_id=123",
        )

    async def test_get_raid_auth_url_passes_scope_profile_query(self) -> None:
        session = _FakeSession(
            response=_FakeResponse(
                status=200,
                text=(
                    '{"ok":true,"auth_url":"https://auth.example/public:website_onboarding",'
                    '"login":"public:website_onboarding"}'
                ),
            )
        )
        client = BotApiClient(
            base_url="http://127.0.0.1:8766",
            token="secret",
            session=session,
        )

        auth_url = await client.get_raid_auth_url(
            "public:website_onboarding",
            scope_profile="base",
        )

        self.assertEqual(auth_url, "https://auth.example/public:website_onboarding")
        self.assertEqual(
            session.calls[0]["url"],
            (
                "http://127.0.0.1:8766/internal/twitch/v1/raid/auth-url"
                "?login=public%3Awebsite_onboarding&scope_profile=base"
            ),
        )

    async def test_get_raid_block_state_supports_login_only(self) -> None:
        session = _FakeSession(
            response=_FakeResponse(
                status=200,
                text='{"ok":true,"twitch_login":"partner_one","blocked":true}',
            )
        )
        client = BotApiClient(
            base_url="http://127.0.0.1:8766",
            token="secret",
            session=session,
        )

        payload = await client.get_raid_block_state(twitch_login="Partner_One")

        self.assertTrue(payload["blocked"])
        self.assertEqual(
            session.calls[0]["url"],
            "http://127.0.0.1:8766/internal/twitch/v1/raid/block-state?twitch_login=partner_one",
        )

    async def test_get_raid_block_state_requires_identifier(self) -> None:
        client = BotApiClient(
            base_url="http://127.0.0.1:8766",
            token="secret",
            session=_FakeSession(response=_FakeResponse(status=200, text="{}")),
        )

        with self.assertRaises(BotApiClientError) as ctx:
            await client.get_raid_block_state()

        self.assertEqual(ctx.exception.status, 400)
        self.assertEqual(ctx.exception.code, "bad_request")

    async def test_disables_redirect_following_to_protect_internal_token(self) -> None:
        session = _FakeSession(response=_FakeResponse(status=200, text='{"ok":true}'))
        client = BotApiClient(
            base_url="http://127.0.0.1:8766",
            token="secret",
            session=session,
        )

        payload = await client.healthz()

        self.assertEqual(payload, {"ok": True})
        self.assertEqual(len(session.calls), 1)
        self.assertIn("kwargs", session.calls[0])
        self.assertFalse(session.calls[0]["kwargs"].get("allow_redirects", True))

    async def test_dashboard_service_marks_analytics_db_fingerprint_mismatch_on_startup(
        self,
    ) -> None:
        with patch(
            "bot.dashboard_service.app.analytics_db_fingerprint_details",
            return_value={
                "fingerprint": "pg:local123456",
                "hostHash": "hosthash",
                "databaseHash": "dbhash",
                "portHash": "porthash",
                "engine": "postgres",
            },
        ):
            app = build_dashboard_service_app(
                noauth=False,
                oauth_client_id="client-id",
                oauth_client_secret="client-secret",
                internal_api_token="secret-token",
                internal_api_base_url="http://127.0.0.1:8776",
            )

        client = app[BOT_API_CLIENT_KEY]
        self.assertIsNotNone(client)

        with patch.object(
            client,
            "healthz",
            return_value={
                "ok": True,
                "analyticsDbFingerprint": "pg:remote654321",
            },
        ):
            for callback in app.on_startup:
                await callback(app)

        self.assertTrue(app[ANALYTICS_DB_FINGERPRINT_MISMATCH_KEY])
        self.assertEqual(app[ANALYTICS_DB_FINGERPRINT_KEY], "pg:local123456")
        self.assertEqual(
            app[INTERNAL_API_ANALYTICS_DB_FINGERPRINT_KEY],
            "pg:remote654321",
        )

        for callback in app.on_cleanup:
            await callback(app)

    async def test_dashboard_service_stats_callback_raises_503_when_upstream_is_unavailable(
        self,
    ) -> None:
        captured_build_kwargs: dict[str, object] = {}

        def _fake_build_v2_app(**kwargs):
            captured_build_kwargs.update(kwargs)
            return web.Application()

        with (
            patch.dict(
                "os.environ",
                {
                    "TWITCH_ALLOW_DASHBOARD_NOAUTH": "1",
                    "TWITCH_DASHBOARD_NOAUTH": "1",
                    "TWITCH_INTERNAL_API_TOKEN": "",
                    "TWITCH_INTERNAL_API_BASE_URL": "http://127.0.0.1:8776",
                },
                clear=False,
            ),
            patch("bot.dashboard_service.app.build_v2_app", side_effect=_fake_build_v2_app),
        ):
            app = build_dashboard_service_app(
                noauth=True,
                oauth_client_id="client-id",
                oauth_client_secret="client-secret",
            )

        self.assertIsInstance(app, web.Application)
        stats_cb = captured_build_kwargs.get("stats_cb")
        self.assertTrue(callable(stats_cb))

        with self.assertRaises(web.HTTPServiceUnavailable) as ctx:
            await stats_cb()

        self.assertEqual(ctx.exception.status, 503)
        self.assertEqual(ctx.exception.text, "Bot internal API is unavailable.")

    def test_rejects_non_loopback_base_url_by_default(self) -> None:
        with self.assertRaises(ValueError) as ctx:
            BotApiClient(
                base_url="http://10.0.0.20:8766",
                token="secret",
            )

        self.assertIn("loopback", str(ctx.exception).lower())

    def test_allows_non_loopback_base_url_when_override_enabled(self) -> None:
        client = BotApiClient(
            base_url="http://10.0.0.20:8766",
            token="secret",
            allow_non_loopback=True,
        )

        self.assertEqual(client._base_url, "http://10.0.0.20:8766")

    def test_dashboard_service_fails_fast_when_noauth_without_allow_override(
        self,
    ) -> None:
        with patch.dict(
            "os.environ",
            {
                "TWITCH_DASHBOARD_NOAUTH": "1",
                "TWITCH_ALLOW_DASHBOARD_NOAUTH": "0",
                "TWITCH_DASHBOARD_HOST": "127.0.0.1",
                "TWITCH_INTERNAL_API_TOKEN": "secret",
                "TWITCH_INTERNAL_API_BASE_URL": "http://127.0.0.1:8766",
            },
            clear=False,
        ):
            with self.assertRaises(RuntimeError) as ctx:
                build_dashboard_service_app()
        self.assertIn("TWITCH_ALLOW_DASHBOARD_NOAUTH=1", str(ctx.exception))

    def test_dashboard_service_allows_noauth_only_with_explicit_override(self) -> None:
        with patch.dict(
            "os.environ",
            {
                "TWITCH_DASHBOARD_NOAUTH": "1",
                "TWITCH_ALLOW_DASHBOARD_NOAUTH": "1",
                "TWITCH_DASHBOARD_HOST": "127.0.0.1",
                "TWITCH_INTERNAL_API_TOKEN": "secret",
                "TWITCH_INTERNAL_API_BASE_URL": "http://127.0.0.1:8766",
            },
            clear=False,
        ):
            app = build_dashboard_service_app()
        self.assertIsNotNone(app)

    def test_dashboard_service_rejects_noauth_on_non_loopback_host(self) -> None:
        with patch.dict(
            "os.environ",
            {
                "TWITCH_DASHBOARD_NOAUTH": "1",
                "TWITCH_ALLOW_DASHBOARD_NOAUTH": "1",
                "TWITCH_DASHBOARD_HOST": "0.0.0.0",
                "TWITCH_INTERNAL_API_TOKEN": "secret",
                "TWITCH_INTERNAL_API_BASE_URL": "http://127.0.0.1:8766",
            },
            clear=False,
        ):
            with self.assertRaises(RuntimeError) as ctx:
                build_dashboard_service_app(noauth=True)

        self.assertIn("loopback", str(ctx.exception).lower())

    def test_dashboard_service_starts_in_degraded_mode_when_internal_token_missing(
        self,
    ) -> None:
        with patch.dict(
            "os.environ",
            {
                "TWITCH_DASHBOARD_NOAUTH": "0",
                "TWITCH_DASHBOARD_HOST": "127.0.0.1",
                "TWITCH_INTERNAL_API_TOKEN": "",
                "TWITCH_INTERNAL_API_BASE_URL": "http://127.0.0.1:8776",
            },
            clear=False,
        ):
            with self.assertLogs("TwitchStreams", level="WARNING") as captured:
                app = build_dashboard_service_app(
                    noauth=False,
                    oauth_client_id="client-id",
                    oauth_client_secret="client-secret",
                )

        self.assertIsNotNone(app)
        degraded_warnings = [
            line
            for line in captured.output
            if "Dashboard service degraded startup" in line
            and "TWITCH_INTERNAL_API_TOKEN missing" in line
        ]
        self.assertEqual(len(degraded_warnings), 1)

    def test_dashboard_service_warns_once_when_oauth_credentials_missing(self) -> None:
        with patch.dict(
            "os.environ",
            {
                "TWITCH_DASHBOARD_NOAUTH": "0",
                "TWITCH_DASHBOARD_HOST": "127.0.0.1",
                "TWITCH_INTERNAL_API_TOKEN": "secret",
                "TWITCH_INTERNAL_API_BASE_URL": "http://127.0.0.1:8776",
                "TWITCH_CLIENT_ID": "",
                "TWITCH_CLIENT_SECRET": "",
            },
            clear=False,
        ):
            with (
                patch("bot.dashboard_service.app.load_secret_value", return_value=""),
                self.assertLogs("TwitchStreams", level="WARNING") as captured,
            ):
                app = build_dashboard_service_app(noauth=False)

        self.assertIsNotNone(app)
        oauth_warnings = [
            line
            for line in captured.output
            if "Dashboard service degraded startup" in line
            and "TWITCH_CLIENT_ID/TWITCH_CLIENT_SECRET missing" in line
        ]
        self.assertEqual(len(oauth_warnings), 1)


if __name__ == "__main__":
    unittest.main()
