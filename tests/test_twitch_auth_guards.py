import contextlib
import asyncio
import unittest
from unittest.mock import patch

from bot.api.twitch_api import TwitchAPI
from bot.api.twitch_auth import TwitchClientConfigError
from bot.raid.auth import RaidAuthManager


class _FakeResponse:
    def __init__(self, *, status: int, text: str = "", payload: dict | None = None) -> None:
        self.status = int(status)
        self._text = text
        self._payload = dict(payload or {})
        self.history = ()
        self.headers = {}
        self.reason = "Bad Request"
        self.request_info = None

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc, tb):
        return False

    async def text(self) -> str:
        return self._text

    async def json(self) -> dict:
        return dict(self._payload)

    def raise_for_status(self) -> None:
        raise RuntimeError(f"unexpected raise_for_status for HTTP {self.status}")


class _RecordingSession:
    def __init__(self, responses: list[_FakeResponse] | None = None) -> None:
        self._responses = list(responses or [])
        self.closed = False
        self.calls: list[dict[str, object]] = []

    def post(self, url, data=None):
        self.calls.append({"url": url, "data": dict(data or {})})
        if not self._responses:
            raise AssertionError("No fake response configured")
        return self._responses.pop(0)


class _CreatedSession:
    def __init__(self) -> None:
        self.closed = False


class _ExplodingSession:
    def __init__(self, exc: BaseException) -> None:
        self.closed = False
        self._exc = exc

    def post(self, *args, **kwargs):
        raise self._exc


class _FakeCursor:
    def __init__(self, rows: list[object] | None = None) -> None:
        self._rows = list(rows or [])

    def fetchall(self) -> list[object]:
        return list(self._rows)


class _RecordingConn:
    def __init__(self) -> None:
        self.calls: list[tuple[str, tuple[object, ...]]] = []

    def execute(self, sql: str, params=()):
        self.calls.append((sql, tuple(params or ())))
        return _FakeCursor()


class TwitchApiAuthGuardTests(unittest.IsolatedAsyncioTestCase):
    async def test_blank_credentials_are_rejected_before_http_call(self) -> None:
        session = _RecordingSession()
        api = TwitchAPI("   ", "   ", session=session)

        with self.assertRaises(TwitchClientConfigError) as ctx:
            await api._ensure_token()

        self.assertIn("missing or blank", str(ctx.exception).lower())
        self.assertEqual(session.calls, [])
        self.assertTrue(api.is_auth_blocked())

    async def test_invalid_client_response_blocks_follow_up_requests(self) -> None:
        session = _RecordingSession(
            responses=[
                _FakeResponse(
                    status=400,
                    text='{"status":400,"message":"invalid client"}',
                )
            ]
        )
        api = TwitchAPI("client-id", "bad-secret", session=session)

        with self.assertRaises(TwitchClientConfigError):
            await api._ensure_token()

        with self.assertRaises(TwitchClientConfigError):
            await api._ensure_token()

        self.assertEqual(len(session.calls), 1)
        self.assertTrue(api.is_auth_blocked())

    def test_http_session_ignores_proxy_env_by_default(self) -> None:
        captured: dict[str, object] = {}

        def _fake_client_session(**kwargs):
            captured.update(kwargs)
            return _CreatedSession()

        with (
            patch.dict("os.environ", {"TWITCH_API_TRUST_ENV": ""}, clear=False),
            patch("bot.api.twitch_api.build_resilient_connector", return_value=object()),
            patch("bot.api.twitch_api.aiohttp.ClientSession", side_effect=_fake_client_session),
        ):
            api = TwitchAPI("client-id", "client-secret")
            api._ensure_session()

        self.assertFalse(bool(captured["trust_env"]))

    def test_http_session_allows_proxy_env_when_explicitly_enabled(self) -> None:
        captured: dict[str, object] = {}

        def _fake_client_session(**kwargs):
            captured.update(kwargs)
            return _CreatedSession()

        with (
            patch.dict("os.environ", {"TWITCH_API_TRUST_ENV": "1"}, clear=False),
            patch("bot.api.twitch_api.build_resilient_connector", return_value=object()),
            patch("bot.api.twitch_api.aiohttp.ClientSession", side_effect=_fake_client_session),
        ):
            api = TwitchAPI("client-id", "client-secret")
            api._ensure_session()

        self.assertTrue(bool(captured["trust_env"]))

    def test_format_exception_summary_uses_class_name_for_empty_message(self) -> None:
        self.assertEqual(
            TwitchAPI._format_exception_summary(asyncio.TimeoutError()),
            "TimeoutError",
        )

    async def test_post_logs_final_timeout_after_retries(self) -> None:
        api = TwitchAPI("client-id", "client-secret", session=_ExplodingSession(TimeoutError()))

        with patch.object(api._log, "error") as error_mock:
            with self.assertRaises(TimeoutError):
                await api._post(
                    "/helix/test",
                    json={"ok": True},
                    oauth_token="oauth:test-token",
                    max_attempts=1,
                )

        error_mock.assert_called_once()
        self.assertIn("failed after retries", error_mock.call_args.args[0])


class RaidAuthManagerAuthGuardTests(unittest.IsolatedAsyncioTestCase):
    async def test_invalid_client_refresh_blocks_follow_up_requests(self) -> None:
        session = _RecordingSession(
            responses=[
                _FakeResponse(
                    status=400,
                    text='{"status":400,"message":"invalid client"}',
                )
            ]
        )
        manager = RaidAuthManager(
            client_id="client-id",
            client_secret="bad-secret",
            redirect_uri="https://raid.example.com/twitch/raid/callback",
        )

        with self.assertRaises(TwitchClientConfigError):
            await manager.refresh_token(
                "refresh-token",
                session,
                twitch_user_id="1001",
                twitch_login="partner_one",
            )

        with self.assertRaises(TwitchClientConfigError):
            await manager.refresh_token(
                "refresh-token",
                session,
                twitch_user_id="1001",
                twitch_login="partner_one",
            )

        self.assertEqual(len(session.calls), 1)
        self.assertTrue(manager.is_client_auth_blocked())

    async def test_refresh_all_tokens_uses_boolean_safe_needs_reauth_filter(self) -> None:
        manager = RaidAuthManager(
            client_id="client-id",
            client_secret="secret",
            redirect_uri="https://raid.example.com/twitch/raid/callback",
        )
        fake_conn = _RecordingConn()

        with patch(
            "bot.raid.auth.readonly_connection",
            side_effect=lambda: contextlib.nullcontext(fake_conn),
        ):
            refreshed = await manager.refresh_all_tokens(_RecordingSession())

        self.assertEqual(refreshed, 0)
        refresh_queries = [
            sql for sql, _ in fake_conn.calls if "FROM twitch_raid_auth" in sql and "SELECT twitch_user_id" in sql
        ]
        self.assertEqual(len(refresh_queries), 1)
        self.assertIn("needs_reauth IS NOT TRUE", refresh_queries[0])
        self.assertNotIn("COALESCE(needs_reauth, 0)", refresh_queries[0])
