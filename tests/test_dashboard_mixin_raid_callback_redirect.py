import unittest
from types import SimpleNamespace

from bot.dashboard.mixin import (
    RAID_OAUTH_SUCCESS_REDIRECT_URL,
    TwitchDashboardMixin,
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

    def __init__(self, *, expected_twitch_login: str = "partner_one", scopes: list[str] | None = None):
        self._expected_twitch_login = expected_twitch_login
        self._scopes = list(scopes or BASE_STREAMER_SCOPES)

    def consume_state_details(self, _state: str):
        return SimpleNamespace(
            requested_login="partner_one",
            scope_profile=BASE_SCOPE_PROFILE,
            expected_twitch_login=self._expected_twitch_login,
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


class _DummyDashboardMixin(TwitchDashboardMixin):
    def __init__(
        self,
        *,
        auth_manager: _FakeAuthManager | None = None,
        user_login: str = "partner_one",
    ) -> None:
        self._raid_bot = SimpleNamespace(
            auth_manager=auth_manager or _FakeAuthManager(),
            session=_FakeSession(payload={"data": [{"id": "1001", "login": user_login}]}),
        )


class DashboardMixinRaidCallbackRedirectTests(unittest.IsolatedAsyncioTestCase):
    async def test_dashboard_callback_payload_contains_redirect_url(self) -> None:
        handler = _DummyDashboardMixin()

        payload = await handler._dashboard_raid_oauth_callback(
            code="oauth-code",
            state="valid-state",
            error="",
        )

        self.assertEqual(payload.get("status"), 200)
        self.assertEqual(payload.get("redirect_url"), RAID_OAUTH_SUCCESS_REDIRECT_URL)

    async def test_dashboard_callback_rejects_wrong_twitch_account(self) -> None:
        handler = _DummyDashboardMixin(user_login="wrong_account")

        payload = await handler._dashboard_raid_oauth_callback(
            code="oauth-code",
            state="valid-state",
            error="",
        )

        self.assertEqual(payload.get("status"), 403)
        self.assertEqual(payload.get("title"), "Falscher Twitch-Account")

    async def test_dashboard_callback_rejects_unexpected_scope_widening(self) -> None:
        handler = _DummyDashboardMixin(
            auth_manager=_FakeAuthManager(
                scopes=list(BASE_STREAMER_SCOPES) + ["channel:read:hype_train"]
            )
        )

        payload = await handler._dashboard_raid_oauth_callback(
            code="oauth-code",
            state="valid-state",
            error="",
        )

        self.assertEqual(payload.get("status"), 400)
        self.assertEqual(payload.get("title"), "Ungültige Berechtigungen")


if __name__ == "__main__":
    unittest.main()
