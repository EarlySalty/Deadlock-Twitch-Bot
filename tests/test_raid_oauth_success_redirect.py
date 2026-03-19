import os
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from aiohttp import web

from bot.dashboard.raid_mixin import (
    DEFAULT_RAID_OAUTH_SUCCESS_REDIRECT_URL,
    _DashboardRaidMixin,
)
from bot.raid.scope_profiles import BASE_SCOPE_PROFILE, BASE_STREAMER_SCOPES


class _FakeUsersResponse:
    def __init__(self, payload: dict):
        self.status = 200
        self._payload = payload

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc, tb):
        return False

    async def text(self) -> str:
        return ""

    async def json(self) -> dict:
        return self._payload


class _FakeSession:
    def __init__(self, payload: dict):
        self._payload = payload

    def get(self, *_args, **_kwargs):
        return _FakeUsersResponse(self._payload)


class _FakeAuthManager:
    client_id = "client-id"
    redirect_uri = "https://raid.earlysalty.com/twitch/raid/callback"

    def __init__(self) -> None:
        self._scopes = list(BASE_STREAMER_SCOPES)

    def consume_state_details(self, _state: str):
        return SimpleNamespace(
            requested_login="partner_one",
            scope_profile=BASE_SCOPE_PROFILE,
            expected_twitch_login="partner_one",
            discord_user_id="123456789",
        )

    def has_saved_auth_record(self, **_kwargs) -> bool:
        return False

    async def exchange_code_for_token(self, _code: str, _session) -> dict:
        return {
            "access_token": "access-token",
            "refresh_token": "refresh-token",
            "expires_in": 3600,
            "scope": list(self._scopes),
        }

    def save_auth(self, **_kwargs) -> None:
        return None


class _DummyRaidCallback(_DashboardRaidMixin):
    def __init__(self) -> None:
        self._raid_bot = SimpleNamespace(
            auth_manager=_FakeAuthManager(),
            session=_FakeSession(payload={"data": [{"id": "1001", "login": "partner_one"}]}),
        )

    @staticmethod
    def _render_oauth_page(title: str, body_html: str) -> str:
        return f"{title}: {body_html}"


class _DummyRaidCallbackProxy(_DashboardRaidMixin):
    def __init__(self, payload: dict) -> None:
        self._raid_bot = None
        self._payload = payload

    async def _raid_oauth_callback_cb(self, *, code: str, state: str, error: str) -> dict:
        del code, state, error
        return dict(self._payload)

    @staticmethod
    def _render_oauth_page(title: str, body_html: str) -> str:
        return f"{title}: {body_html}"


class RaidOAuthSuccessRedirectTests(unittest.IsolatedAsyncioTestCase):
    def test_success_redirect_default(self) -> None:
        with patch.dict(os.environ, {}, clear=False):
            self.assertEqual(
                _DashboardRaidMixin._raid_oauth_success_redirect_url(),
                DEFAULT_RAID_OAUTH_SUCCESS_REDIRECT_URL,
            )

    def test_success_redirect_env_override_https(self) -> None:
        with patch.dict(
            os.environ,
            {"TWITCH_RAID_SUCCESS_REDIRECT_URL": "https://dashboard.example/twitch/dashboard"},
            clear=False,
        ):
            self.assertEqual(
                _DashboardRaidMixin._raid_oauth_success_redirect_url(),
                "https://dashboard.example/twitch/dashboard",
            )

    def test_success_redirect_rejects_untrusted_http_host(self) -> None:
        with patch.dict(
            os.environ,
            {"TWITCH_RAID_SUCCESS_REDIRECT_URL": "http://evil.example/twitch/dashboard"},
            clear=False,
        ):
            self.assertEqual(
                _DashboardRaidMixin._raid_oauth_success_redirect_url(),
                DEFAULT_RAID_OAUTH_SUCCESS_REDIRECT_URL,
            )

    async def test_raid_callback_redirects_to_dashboard_on_success(self) -> None:
        handler = _DummyRaidCallback()
        request = SimpleNamespace(query={"code": "oauth-code", "state": "valid-state"})

        with self.assertRaises(web.HTTPFound) as ctx:
            await handler.raid_oauth_callback(request)

        self.assertEqual(ctx.exception.location, DEFAULT_RAID_OAUTH_SUCCESS_REDIRECT_URL)

    async def test_raid_callback_proxy_payload_redirects_to_dashboard(self) -> None:
        handler = _DummyRaidCallbackProxy(
            {
                "status": 200,
                "title": "Autorisierung erfolgreich",
                "body_html": "<p>ok</p>",
                "redirect_url": "https://twitch.earlysalty.com/twitch/dashboard",
            }
        )
        request = SimpleNamespace(query={"code": "oauth-code", "state": "valid-state"})

        with self.assertRaises(web.HTTPFound) as ctx:
            await handler.raid_oauth_callback(request)

        self.assertEqual(ctx.exception.location, DEFAULT_RAID_OAUTH_SUCCESS_REDIRECT_URL)

    async def test_raid_callback_proxy_redirect_uses_safe_fallback_for_invalid_url(self) -> None:
        handler = _DummyRaidCallbackProxy(
            {
                "status": 200,
                "title": "Autorisierung erfolgreich",
                "body_html": "<p>ok</p>",
                "redirect_url": "javascript:alert(1)",
            }
        )
        request = SimpleNamespace(query={"code": "oauth-code", "state": "valid-state"})

        with self.assertRaises(web.HTTPFound) as ctx:
            await handler.raid_oauth_callback(request)

        self.assertEqual(ctx.exception.location, DEFAULT_RAID_OAUTH_SUCCESS_REDIRECT_URL)


if __name__ == "__main__":
    unittest.main()
